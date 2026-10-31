//! Codex rollouts → normalized events.
//!
//! Each thread writes `sessions/YYYY/MM/DD/rollout-<time>-<thread id>.jsonl`,
//! one `{ "timestamp", "type", "payload" }` object per line. The first line
//! is `session_meta`: `id` is the thread, `session_id` the root session, so a
//! subagent's own rollout names both. Later chunks re-read that first line to
//! know whose file they are.

use std::io::{BufRead, BufReader};
use std::path::Path;

use agentscope_core::{
    AgentEvent, AgentResult, EventKind, EventSource, HarnessId, Timestamp, Tokens,
};
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::summary::{tool_files, tool_summary};
use crate::TranscriptFile;

/// Longest first line read to learn a file's owner; `session_meta` can carry
/// the base instructions, so it is not tiny.
const META_MAX_BYTES: u64 = 1024 * 1024;

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn timestamp(v: &Value) -> Option<Timestamp> {
    DateTime::parse_from_rfc3339(str_of(v, "timestamp")?)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// The session and agent a rollout belongs to, from its `session_meta`.
#[derive(Debug, PartialEq, Eq)]
struct Owner {
    session: String,
    /// `None` for the root thread.
    agent: Option<String>,
    parent: Option<String>,
}

fn owner(meta: &Value) -> Option<Owner> {
    if str_of(meta, "type") != Some("session_meta") {
        return None;
    }
    let p = meta.get("payload")?;
    let thread = str_of(p, "id")?;
    let session = str_of(p, "session_id").unwrap_or(thread);
    let agent = (thread != session).then(|| thread.to_owned());
    let parent = str_of(p, "parent_thread_id").filter(|t| *t != session);
    Some(Owner {
        session: session.to_owned(),
        agent,
        parent: parent.map(str::to_owned),
    })
}

fn first_line(path: &Path) -> Option<Value> {
    let file = std::fs::File::open(path).ok()?;
    let mut line = Vec::new();
    BufReader::new(std::io::Read::take(file, META_MAX_BYTES))
        .read_until(b'\n', &mut line)
        .ok()?;
    serde_json::from_slice(&line).ok()
}

/// `["bash", "-lc", "npm test"]` reads as `npm test`.
fn command_text(command: &Value) -> Option<String> {
    match command {
        Value::String(s) => Some(s.clone()),
        Value::Array(parts) => {
            let parts: Vec<&str> = parts.iter().filter_map(Value::as_str).collect();
            match parts.as_slice() {
                [_, flag, script] if flag.starts_with('-') && flag.ends_with('c') => {
                    Some((*script).to_owned())
                }
                _ => Some(parts.join(" ")),
            }
        }
        _ => None,
    }
}

struct Parser {
    owner: Owner,
    events: Vec<AgentEvent>,
    at: Option<Timestamp>,
    turn: Option<String>,
    cwd: String,
}

impl Parser {
    fn push(&mut self, key: Option<String>, kind: EventKind) {
        let Some(at) = self.at else { return };
        self.events.push(AgentEvent {
            source: EventSource::Transcript,
            at,
            harness: HarnessId::Codex,
            session: self.owner.session.clone(),
            agent: self.owner.agent.clone(),
            dedupe_key: key,
            kind,
        });
    }

    fn is_main(&self) -> bool {
        self.owner.agent.is_none()
    }

    fn line(&mut self, file: &Path, line: &Value) {
        if let Some(t) = timestamp(line) {
            self.at = Some(t);
        }
        let p = line.get("payload").unwrap_or(&Value::Null);
        match str_of(line, "type") {
            Some("session_meta") => self.meta(file, p),
            Some("turn_context") => {
                if let Some(turn) = str_of(p, "turn_id") {
                    self.turn = Some(turn.to_owned());
                }
                if let Some(cwd) = str_of(p, "cwd") {
                    self.cwd = cwd.to_owned();
                }
                if self.is_main() {
                    let kind = EventKind::SessionInfo {
                        cwd: str_of(p, "cwd").map(str::to_owned),
                        model: str_of(p, "model").map(str::to_owned),
                        title: None,
                    };
                    self.push(None, kind);
                }
            }
            Some("response_item") => self.response_item(p),
            Some("event_msg") => self.event_msg(p),
            Some("compacted") => self.push(None, EventKind::Compacted),
            _ => {}
        }
    }

    fn meta(&mut self, file: &Path, p: &Value) {
        if let Some(cwd) = str_of(p, "cwd") {
            self.cwd = cwd.to_owned();
        }
        let kind = match &self.owner.agent {
            None => EventKind::SessionStarted {
                cwd: self.cwd.clone(),
                model: None,
                transcript_path: Some(file.display().to_string()),
                pid: None,
            },
            Some(_) => EventKind::AgentStarted {
                parent: self.owner.parent.clone(),
                agent_type: str_of(p, "agent_role")
                    .or_else(|| str_of(p, "agent_type"))
                    .map(str::to_owned),
                description: str_of(p, "agent_nickname").map(str::to_owned),
            },
        };
        self.push(Some("transcript-start".into()), kind);
    }

    fn response_item(&mut self, p: &Value) {
        let call_id = str_of(p, "call_id").map(str::to_owned);
        let started = |tool: &str, input: Value| (tool.to_owned(), input);
        let call = match str_of(p, "type") {
            Some("function_call") => {
                let name = str_of(p, "name").unwrap_or("tool");
                let args = str_of(p, "arguments")
                    .and_then(|a| serde_json::from_str::<Value>(a).ok())
                    .unwrap_or(Value::Null);
                // Shell tools take `cmd` or a `command` argv.
                let input = match args.get("command").and_then(command_text) {
                    Some(cmd) => serde_json::json!({ "command": cmd }),
                    None => args,
                };
                let tool = match name {
                    "shell" | "exec_command" | "shell_command" | "local_shell" => "Bash",
                    other => other,
                };
                Some(started(tool, input))
            }
            Some("custom_tool_call") => {
                let name = str_of(p, "name").unwrap_or("tool");
                let input =
                    serde_json::json!({ "command": str_of(p, "input").unwrap_or_default() });
                Some(started(name, input))
            }
            Some("local_shell_call") => {
                let cmd = p.pointer("/action/command").and_then(command_text);
                Some(started("Bash", serde_json::json!({ "command": cmd })))
            }
            Some("function_call_output" | "custom_tool_call_output") => {
                self.push(call_id.clone(), EventKind::ToolFinished);
                None
            }
            _ => None,
        };
        if let Some((tool, input)) = call {
            let summary = tool_summary(&tool, &input, &self.cwd);
            let files = tool_files(&tool, &input, &self.cwd);
            self.push(
                call_id,
                EventKind::ToolStarted {
                    tool,
                    summary,
                    files,
                },
            );
        }
    }

    fn event_msg(&mut self, p: &Value) {
        match str_of(p, "type") {
            Some("task_started" | "turn_started") => {
                if let Some(turn) = str_of(p, "turn_id") {
                    self.turn = Some(turn.to_owned());
                }
            }
            Some("user_message") if self.is_main() => {
                let Some(prompt) = str_of(p, "message") else {
                    return;
                };
                // Codex wraps injected context in tags; that is not a prompt.
                if prompt.trim_start().starts_with('<') {
                    return;
                }
                let kind = EventKind::PromptSubmitted {
                    prompt: prompt.to_owned(),
                };
                self.push(self.turn.clone(), kind);
            }
            Some("task_complete" | "turn_complete") => {
                let key = str_of(p, "turn_id")
                    .map(str::to_owned)
                    .or_else(|| self.turn.clone())
                    .map(|t| format!("turn-end-{t}"));
                let kind = if self.is_main() {
                    EventKind::TurnCompleted
                } else {
                    EventKind::AgentStopped {
                        result: AgentResult::Completed,
                    }
                };
                self.push(key, kind);
            }
            Some("turn_aborted") => {
                let kind = if self.is_main() {
                    EventKind::TurnCompleted
                } else {
                    EventKind::AgentStopped {
                        result: AgentResult::Aborted,
                    }
                };
                self.push(None, kind);
            }
            Some("token_count") if self.is_main() => {
                let Some(total) = p.pointer("/info/total_token_usage") else {
                    return;
                };
                let n = |k: &str| total.get(k).and_then(Value::as_u64).unwrap_or(0);
                let cached = n("cached_input_tokens");
                let tokens = Tokens {
                    input: n("input_tokens").saturating_sub(cached),
                    output: n("output_tokens"),
                    cached: Some(cached),
                };
                if tokens.input + tokens.output > 0 {
                    self.push(None, EventKind::TokensReported { tokens });
                }
            }
            _ => {}
        }
    }
}

pub fn parse(file: &TranscriptFile, bytes: &[u8]) -> Vec<AgentEvent> {
    let lines: Vec<Value> = bytes
        .split(|&b| b == b'\n')
        .filter_map(|l| serde_json::from_slice(l).ok())
        .collect();
    let meta = match file.offset {
        0 => lines.first().cloned(),
        _ => first_line(&file.path),
    };
    let Some(owner) = meta.as_ref().and_then(owner) else {
        return Vec::new();
    };
    let mut parser = Parser {
        owner,
        events: Vec::new(),
        at: lines.iter().find_map(timestamp),
        turn: None,
        cwd: meta
            .as_ref()
            .and_then(|m| m.pointer("/payload/cwd"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    };
    for line in &lines {
        parser.line(&file.path, line);
    }
    parser.events
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_the_owner_from_session_meta() {
        let root = json!({ "type": "session_meta", "payload": { "id": "t1", "session_id": "t1" } });
        assert_eq!(
            owner(&root),
            Some(Owner {
                session: "t1".into(),
                agent: None,
                parent: None
            })
        );
        let old = json!({ "type": "session_meta", "payload": { "id": "t1" } });
        assert_eq!(owner(&old).unwrap().session, "t1");
        let sub = json!({ "type": "session_meta",
            "payload": { "id": "t3", "session_id": "t1", "parent_thread_id": "t2" } });
        assert_eq!(
            owner(&sub),
            Some(Owner {
                session: "t1".into(),
                agent: Some("t3".into()),
                parent: Some("t2".into())
            })
        );
        assert!(owner(&json!({ "type": "event_msg" })).is_none());
    }

    #[test]
    fn shell_argv_reads_as_the_script() {
        assert_eq!(
            command_text(&json!(["bash", "-lc", "npm test"])).as_deref(),
            Some("npm test")
        );
        assert_eq!(
            command_text(&json!(["ls", "-la"])).as_deref(),
            Some("ls -la")
        );
        assert_eq!(
            command_text(&json!("git status")).as_deref(),
            Some("git status")
        );
    }
}
