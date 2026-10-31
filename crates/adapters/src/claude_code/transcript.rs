//! Claude Code transcripts → normalized events.
//!
//! Layout under `~/.claude/projects/<encoded-cwd>/`:
//!
//! - `<session>.jsonl`: the main agent, one JSON object per line.
//! - `<session>/subagents/agent-<id>.jsonl` plus `agent-<id>.meta.json`
//!   (`agentType`, `description`): one file per subagent.
//!
//! Older versions wrote subagent lines into the main file with
//! `isSidechain: true`; those are attributed by their `agentId` when present.
//! Hooks are the live signal; this fills in sessions started before AgentScope
//! was connected, token counts, models, and titles.

use std::path::Path;

use agentscope_core::{
    AgentEvent, AgentResult, EventKind, EventSource, HarnessId, Timestamp, Tokens,
};
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::summary::{tool_files, tool_summary};
use crate::TranscriptFile;

/// Which agent a transcript file belongs to, from its path.
struct FileOwner {
    session: String,
    /// `None` for the main file.
    agent: Option<String>,
}

fn owner(path: &Path) -> Option<FileOwner> {
    let stem = path.file_stem()?.to_str()?;
    let parent = path.parent()?;
    if parent.file_name()? == "subagents" {
        let session = parent.parent()?.file_name()?.to_str()?.to_owned();
        let agent = stem.strip_prefix("agent-").unwrap_or(stem).to_owned();
        return Some(FileOwner {
            session,
            agent: Some(agent),
        });
    }
    // Main transcripts sit directly in the project folder.
    if parent.parent()?.file_name()? != "projects" {
        return None;
    }
    Some(FileOwner {
        session: stem.to_owned(),
        agent: None,
    })
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn timestamp(v: &Value) -> Option<Timestamp> {
    DateTime::parse_from_rfc3339(str_of(v, "timestamp")?)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// Prompts Claude Code itself injects: slash command echoes, hook output,
/// interrupt markers. They are not something the user asked.
fn is_synthetic_prompt(text: &str) -> bool {
    let t = text.trim_start();
    t.is_empty() || t.starts_with('<') || t.starts_with("[Request interrupted")
}

struct Parser<'a> {
    owner: FileOwner,
    file: &'a TranscriptFile,
    events: Vec<AgentEvent>,
    /// The latest timestamp seen, for lines without one.
    at: Option<Timestamp>,
    cwd: Option<String>,
    model: Option<String>,
    title: Option<String>,
}

impl Parser<'_> {
    fn push(&mut self, session: &str, agent: Option<String>, key: Option<String>, kind: EventKind) {
        let Some(at) = self.at else { return };
        self.events.push(AgentEvent {
            source: EventSource::Transcript,
            at,
            harness: HarnessId::ClaudeCode,
            session: session.to_owned(),
            agent,
            dedupe_key: key,
            kind,
        });
    }

    fn line(&mut self, line: &Value) {
        if let Some(t) = timestamp(line) {
            self.at = Some(t);
        }
        let session = str_of(line, "sessionId")
            .unwrap_or(&self.owner.session)
            .to_owned();
        let agent = match (
            &self.owner.agent,
            line.get("isSidechain").and_then(Value::as_bool),
        ) {
            (Some(agent), _) => Some(agent.clone()),
            (None, Some(true)) => match str_of(line, "agentId") {
                Some(id) => Some(id.to_owned()),
                // An old inline subagent line we cannot attribute.
                None => return,
            },
            (None, _) => None,
        };
        if let Some(cwd) = str_of(line, "cwd") {
            self.cwd = Some(cwd.to_owned());
        }

        match str_of(line, "type") {
            Some("custom-title") => self.title = str_of(line, "customTitle").map(str::to_owned),
            Some("ai-title") if self.title.is_none() => {
                self.title = str_of(line, "aiTitle").map(str::to_owned)
            }
            Some("user") if !line.get("isMeta").and_then(Value::as_bool).unwrap_or(false) => {
                self.user(line, &session, agent)
            }
            Some("assistant") => self.assistant(line, &session, agent),
            _ => {}
        }
    }

    fn user(&mut self, line: &Value, session: &str, agent: Option<String>) {
        let content = line.pointer("/message/content").unwrap_or(&Value::Null);
        let prompt_key = str_of(line, "promptId").map(str::to_owned);
        match content {
            Value::String(text) if agent.is_none() && !is_synthetic_prompt(text) => {
                self.push(
                    session,
                    agent,
                    prompt_key,
                    EventKind::PromptSubmitted {
                        prompt: text.clone(),
                    },
                );
            }
            Value::Array(items) => {
                let mut text = String::new();
                for item in items {
                    match str_of(item, "type") {
                        Some("tool_result") => {
                            let failed =
                                item.get("is_error").and_then(Value::as_bool) == Some(true);
                            let kind = if failed {
                                EventKind::ToolFailed { error: None }
                            } else {
                                EventKind::ToolFinished
                            };
                            let key = str_of(item, "tool_use_id").map(str::to_owned);
                            self.push(session, agent.clone(), key, kind);
                        }
                        Some("text") => text.push_str(str_of(item, "text").unwrap_or_default()),
                        _ => {}
                    }
                }
                if agent.is_none() && !is_synthetic_prompt(&text) {
                    self.push(
                        session,
                        None,
                        prompt_key,
                        EventKind::PromptSubmitted { prompt: text },
                    );
                }
            }
            _ => {}
        }
    }

    fn assistant(&mut self, line: &Value, session: &str, agent: Option<String>) {
        let Some(message) = line.get("message") else {
            return;
        };
        let message_id = str_of(message, "id").map(str::to_owned);
        if let Some(model) = str_of(message, "model").filter(|m| !m.starts_with('<')) {
            if agent.is_none() {
                self.model = Some(model.to_owned());
            }
        }
        let cwd = self.cwd.clone().unwrap_or_default();
        for item in message
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if str_of(item, "type") != Some("tool_use") {
                continue;
            }
            let tool = str_of(item, "name").unwrap_or("tool");
            let input = item.get("input").unwrap_or(&Value::Null);
            self.push(
                session,
                agent.clone(),
                str_of(item, "id").map(str::to_owned),
                EventKind::ToolStarted {
                    tool: tool.to_owned(),
                    summary: tool_summary(tool, input, &cwd),
                    files: tool_files(tool, input, &cwd),
                },
            );
        }
        // A message is split over several lines that repeat its usage, so the
        // message id dedupes them.
        if let Some(usage) = message.get("usage") {
            let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
            let tokens = Tokens {
                input: n("input_tokens") + n("cache_creation_input_tokens"),
                output: n("output_tokens"),
                cached: Some(n("cache_read_input_tokens")),
            };
            if tokens.input + tokens.output > 0 {
                self.push(
                    session,
                    agent.clone(),
                    message_id.clone(),
                    EventKind::TokensUsed { tokens },
                );
            }
        }
        if str_of(message, "stop_reason") == Some("end_turn") {
            let kind = match agent {
                Some(_) => EventKind::AgentStopped {
                    result: AgentResult::Completed,
                },
                None => EventKind::TurnCompleted,
            };
            self.push(session, agent, message_id, kind);
        }
    }
}

pub fn parse(file: &TranscriptFile, bytes: &[u8]) -> Vec<AgentEvent> {
    let Some(owner) = owner(&file.path) else {
        return Vec::new();
    };
    let mut parser = Parser {
        owner,
        file,
        events: Vec::new(),
        at: None,
        cwd: None,
        model: None,
        title: None,
    };
    let lines: Vec<Value> = bytes
        .split(|&b| b == b'\n')
        .filter_map(|l| serde_json::from_slice(l).ok())
        .collect();

    if parser.file.offset == 0 {
        parser.at = lines.iter().find_map(timestamp);
        parser.start(&lines);
    }
    for line in &lines {
        parser.line(line);
    }
    parser.finish();
    parser.events
}

impl Parser<'_> {
    /// The first chunk of a file announces the session or the subagent.
    fn start(&mut self, lines: &[Value]) {
        let session = self.owner.session.clone();
        match self.owner.agent.clone() {
            None => {
                let cwd = lines
                    .iter()
                    .find_map(|l| str_of(l, "cwd"))
                    .unwrap_or_default();
                self.push(
                    &session,
                    None,
                    Some("transcript-start".into()),
                    EventKind::SessionStarted {
                        cwd: cwd.to_owned(),
                        model: None,
                        transcript_path: Some(self.file.path.display().to_string()),
                        pid: None,
                    },
                );
            }
            Some(agent) => {
                let meta = self
                    .file
                    .path
                    .with_file_name(format!("agent-{agent}.meta.json"));
                let meta: Value = std::fs::read(meta)
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok())
                    .unwrap_or(Value::Null);
                self.push(
                    &session,
                    Some(agent),
                    None,
                    EventKind::AgentStarted {
                        parent: None,
                        agent_type: str_of(&meta, "agentType").map(str::to_owned),
                        description: str_of(&meta, "description").map(str::to_owned),
                    },
                );
            }
        }
    }

    /// One `SessionInfo` per chunk with what changed, instead of one per line.
    fn finish(&mut self) {
        if self.owner.agent.is_some()
            || (self.cwd.is_none() && self.model.is_none() && self.title.is_none())
        {
            return;
        }
        let session = self.owner.session.clone();
        let kind = EventKind::SessionInfo {
            cwd: self.cwd.take(),
            model: self.model.take(),
            title: self.title.take(),
        };
        self.push(&session, None, None, kind);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_main_and_subagent_files() {
        let main = owner(Path::new("/h/.claude/projects/-repo/abc.jsonl")).unwrap();
        assert_eq!((main.session.as_str(), main.agent), ("abc", None));
        let sub = owner(Path::new(
            "/h/.claude/projects/-repo/abc/subagents/agent-a1.jsonl",
        ))
        .unwrap();
        assert_eq!(
            (sub.session.as_str(), sub.agent.as_deref()),
            ("abc", Some("a1"))
        );
        assert!(owner(Path::new(
            "/h/.claude/projects/-repo/abc/tool-results/x.jsonl"
        ))
        .is_none());
    }
}
