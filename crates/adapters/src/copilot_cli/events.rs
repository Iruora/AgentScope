//! Copilot CLI `events.jsonl` → normalized events.
//!
//! `~/.copilot/session-state/<session>/events.jsonl` is the session's own
//! append-only log: `{ "type", "data", "id", "timestamp", "parentId" }`, plus
//! `agentId` on events from a subagent. Copilot writes it as things happen,
//! so it is a live source too. Transient events (deltas, `session.idle`,
//! `assistant.usage`) are never written to it.

use std::path::Path;

use agentscope_core::{AgentEvent, EventKind, EventSource, HarnessId, Timestamp, Tokens};
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::summary::{tool_files, tool_summary};
use crate::TranscriptFile;

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn timestamp(v: &Value) -> Option<Timestamp> {
    DateTime::parse_from_rfc3339(str_of(v, "timestamp")?)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// `session-state/<session>/events.jsonl` → `<session>`.
fn session_of(path: &Path) -> Option<&str> {
    if path.file_name()? != "events.jsonl" {
        return None;
    }
    path.parent()?.file_name()?.to_str()
}

pub fn parse(file: &TranscriptFile, bytes: &[u8]) -> Vec<AgentEvent> {
    let Some(session) = session_of(&file.path) else {
        return Vec::new();
    };
    let mut events = Vec::new();
    let mut cwd = String::new();
    // A main-agent message without tool requests means the turn that ends
    // next is the last one before the user's turn.
    let mut answered = false;

    for line in bytes
        .split(|&b| b == b'\n')
        .filter_map(|l| serde_json::from_slice::<Value>(l).ok())
    {
        let Some(at) = timestamp(&line) else { continue };
        let agent = str_of(&line, "agentId").map(str::to_owned);
        let data = line.get("data").unwrap_or(&Value::Null);
        let mut push = |agent: Option<String>, key: Option<String>, kind: EventKind| {
            events.push(AgentEvent {
                source: EventSource::Transcript,
                at,
                harness: HarnessId::CopilotCli,
                session: session.to_owned(),
                agent,
                dedupe_key: key,
                kind,
            })
        };
        match str_of(&line, "type") {
            Some(kind @ ("session.start" | "session.resume")) => {
                if let Some(dir) = data.pointer("/context/cwd").and_then(Value::as_str) {
                    cwd = dir.to_owned();
                }
                let model = str_of(data, "selectedModel").map(str::to_owned);
                if kind == "session.start" {
                    push(
                        None,
                        Some("transcript-start".into()),
                        EventKind::SessionStarted {
                            cwd: cwd.clone(),
                            model,
                            transcript_path: Some(file.path.display().to_string()),
                            pid: None,
                        },
                    );
                } else {
                    let info = EventKind::SessionInfo {
                        cwd: (!cwd.is_empty()).then(|| cwd.clone()),
                        model,
                        title: None,
                    };
                    push(None, None, info);
                }
            }
            Some("session.model_change") => push(
                None,
                None,
                EventKind::SessionInfo {
                    cwd: None,
                    model: str_of(data, "newModel").map(str::to_owned),
                    title: None,
                },
            ),
            Some("session.shutdown") => {
                let mut tokens = Tokens::default();
                for metric in data
                    .get("modelMetrics")
                    .and_then(Value::as_object)
                    .into_iter()
                    .flat_map(|m| m.values())
                {
                    let n = |k: &str| {
                        metric
                            .pointer(&format!("/usage/{k}"))
                            .and_then(Value::as_u64)
                            .unwrap_or(0)
                    };
                    tokens.input += n("inputTokens") + n("cacheWriteTokens");
                    tokens.output += n("outputTokens");
                    tokens.cached = Some(tokens.cached.unwrap_or(0) + n("cacheReadTokens"));
                }
                if tokens.input + tokens.output > 0 {
                    push(None, None, EventKind::TokensReported { tokens });
                }
                push(None, None, EventKind::SessionEnded);
            }
            Some("user.message") if agent.is_none() => {
                let Some(prompt) = str_of(data, "content") else {
                    continue;
                };
                let key = str_of(&line, "id").map(str::to_owned);
                let kind = EventKind::PromptSubmitted {
                    prompt: prompt.to_owned(),
                };
                push(None, key, kind);
            }
            Some("assistant.message") if agent.is_none() => {
                answered = data
                    .get("toolRequests")
                    .and_then(Value::as_array)
                    .is_none_or(|r| r.is_empty());
            }
            Some("assistant.turn_end") if agent.is_none() && answered => {
                answered = false;
                let key = str_of(data, "turnId").map(|t| format!("turn-end-{t}"));
                push(None, key, EventKind::TurnCompleted);
            }
            Some("tool.execution_start") => {
                let tool = str_of(data, "toolName").unwrap_or("tool");
                let input = data.get("arguments").unwrap_or(&Value::Null);
                let kind = EventKind::ToolStarted {
                    tool: tool.to_owned(),
                    summary: tool_summary(tool, input, &cwd),
                    files: tool_files(tool, input, &cwd),
                };
                push(agent, str_of(data, "toolCallId").map(str::to_owned), kind);
            }
            Some("tool.execution_complete") => {
                let ok = data.get("success").and_then(Value::as_bool).unwrap_or(true);
                let kind = if ok {
                    EventKind::ToolFinished
                } else {
                    EventKind::ToolFailed {
                        error: data
                            .pointer("/error/message")
                            .and_then(Value::as_str)
                            .map(|e| agentscope_core::redact::truncate(e, 300)),
                    }
                };
                push(agent, str_of(data, "toolCallId").map(str::to_owned), kind);
            }
            Some("session.compaction_start") => push(agent, None, EventKind::Compacted),
            _ => {}
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_events_files_belong_to_a_session() {
        assert_eq!(
            session_of(Path::new("/h/.copilot/session-state/abc/events.jsonl")),
            Some("abc")
        );
        assert_eq!(
            session_of(Path::new("/h/.copilot/session-state/abc/other.jsonl")),
            None
        );
    }
}
