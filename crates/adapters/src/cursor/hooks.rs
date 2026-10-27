//! Cursor hook payloads → normalized events.
//!
//! Every payload carries `conversation_id`, `generation_id`,
//! `hook_event_name`, `workspace_roots` and `model`. The conversation is the
//! session. A subagent gets its own conversation (`subagent_id` in
//! `subagentStart`), which is mapped back onto its parent session.
//!
//! Cursor reports no permission prompt. A shell or MCP call is reported as an
//! [`EventKind::ApprovalPossible`]; the core turns it into an inferred
//! approval when no result follows within a few seconds.

use std::collections::HashMap;

use agentscope_core::redact::{redact_json, truncate};
use agentscope_core::{
    AgentEvent, AgentResult, EventKind, EventSource, HarnessId, RawHookEvent, Resolution,
};
use chrono::{TimeZone, Utc};
use serde_json::Value;

use crate::summary::{tool_summary, DETAIL_MAX_STRING};

/// Hook events AgentScope registers. The shim prints nothing, which Cursor
/// treats as "no decision" and lets the action proceed, so none of these
/// can approve, deny, or block anything.
pub const EVENTS: &[&str] = &[
    "sessionStart",
    "sessionEnd",
    "beforeSubmitPrompt",
    "preToolUse",
    "postToolUse",
    "postToolUseFailure",
    "subagentStart",
    "subagentStop",
    "stop",
    "preCompact",
];

/// A running subagent and the session it belongs to.
#[derive(Debug, Clone)]
pub struct Subagent {
    session: String,
    agent_type: Option<String>,
    task: Option<String>,
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// Tools that can stop for the user's approval.
fn may_prompt(tool: &str) -> bool {
    let t = tool.to_ascii_lowercase();
    t == "shell" || t == "run_terminal_cmd" || t.starts_with("mcp")
}

/// `tool_input` is an object for built-in tools and a JSON string for MCP.
fn tool_input(p: &Value) -> Value {
    match p.get("tool_input") {
        Some(Value::String(s)) => serde_json::from_str(s).unwrap_or(Value::String(s.clone())),
        Some(v) => v.clone(),
        None => Value::Null,
    }
}

pub fn normalize(raw: &RawHookEvent, subagents: &mut HashMap<String, Subagent>) -> Vec<AgentEvent> {
    let p = &raw.payload;
    let Some(conversation) = str_of(p, "conversation_id").or_else(|| str_of(p, "session_id"))
    else {
        return Vec::new();
    };
    let at = Utc
        .timestamp_millis_opt(raw.received_at as i64)
        .single()
        .unwrap_or_else(Utc::now);
    let (session, agent) = match subagents.get(conversation) {
        Some(sub) => (sub.session.clone(), Some(conversation.to_owned())),
        None => (conversation.to_owned(), None),
    };
    let cwd = str_of(p, "cwd")
        .or_else(|| p.pointer("/workspace_roots/0").and_then(Value::as_str))
        .unwrap_or_default();
    let tool = str_of(p, "tool_name").unwrap_or("tool");
    let tool_use_id = str_of(p, "tool_use_id").map(str::to_owned);

    let event = |agent: Option<String>, dedupe_key: Option<String>, kind: EventKind| AgentEvent {
        source: EventSource::Hook,
        at,
        harness: HarnessId::Cursor,
        session: session.clone(),
        agent,
        dedupe_key,
        kind,
    };
    let mut out = Vec::new();
    match raw.event.as_str() {
        "sessionStart" if agent.is_none() => out.push(event(
            None,
            None,
            EventKind::SessionStarted {
                cwd: cwd.to_owned(),
                model: str_of(p, "model").map(str::to_owned),
                transcript_path: str_of(p, "transcript_path").map(str::to_owned),
                pid: None,
            },
        )),
        "sessionEnd" if agent.is_none() => {
            subagents.retain(|_, s| s.session != session);
            out.push(event(None, None, EventKind::SessionEnded));
        }
        "beforeSubmitPrompt" => out.push(event(
            agent.clone(),
            str_of(p, "generation_id").map(str::to_owned),
            EventKind::PromptSubmitted {
                prompt: str_of(p, "prompt").unwrap_or_default().to_owned(),
            },
        )),
        "preToolUse" => {
            let input = tool_input(p);
            let summary = tool_summary(tool, &input, cwd);
            out.push(event(
                agent.clone(),
                tool_use_id.clone(),
                EventKind::ToolStarted {
                    tool: tool.to_owned(),
                    summary: summary.clone(),
                },
            ));
            if may_prompt(tool) {
                out.push(event(
                    agent.clone(),
                    None,
                    EventKind::ApprovalPossible {
                        approval_id: tool_use_id
                            .clone()
                            .unwrap_or_else(|| format!("tool-{}", raw.received_at)),
                        tool: tool.to_owned(),
                        summary,
                        detail: (!input.is_null()).then(|| redact_json(&input, DETAIL_MAX_STRING)),
                    },
                ));
            }
        }
        "postToolUse" => out.push(event(agent.clone(), tool_use_id, EventKind::ToolFinished)),
        "postToolUseFailure" => {
            if str_of(p, "failure_type") == Some("permission_denied") {
                out.push(event(
                    agent.clone(),
                    None,
                    EventKind::ApprovalResolved {
                        approval_id: None,
                        resolution: Resolution::Denied,
                    },
                ));
            }
            out.push(event(
                agent.clone(),
                tool_use_id,
                EventKind::ToolFailed {
                    error: str_of(p, "error_message").map(|e| truncate(e, 300)),
                },
            ));
        }
        "subagentStart" => {
            let Some(id) = str_of(p, "subagent_id") else {
                return out;
            };
            let sub = Subagent {
                session: session.clone(),
                agent_type: str_of(p, "subagent_type").map(str::to_owned),
                task: str_of(p, "task").map(str::to_owned),
            };
            out.push(event(
                Some(id.to_owned()),
                None,
                EventKind::AgentStarted {
                    // A subagent's own subagent hangs under it.
                    parent: agent.clone(),
                    agent_type: sub.agent_type.clone(),
                    description: sub.task.as_deref().map(|t| truncate(t, 200)),
                },
            ));
            subagents.insert(id.to_owned(), sub);
        }
        "subagentStop" => {
            // Sent from the subagent's conversation, or from the parent's
            // with only the type and task to go by.
            let id = agent.clone().or_else(|| {
                let task = str_of(p, "task");
                let kind = str_of(p, "subagent_type");
                subagents
                    .iter()
                    .filter(|(_, s)| s.session == session)
                    .find(|(_, s)| {
                        (task.is_some() && s.task.as_deref() == task)
                            || (task.is_none() && s.agent_type.as_deref() == kind)
                    })
                    .map(|(id, _)| id.clone())
            });
            if let Some(id) = id {
                subagents.remove(&id);
                let result = match str_of(p, "status") {
                    Some("error") => AgentResult::Failed,
                    Some("aborted") => AgentResult::Aborted,
                    _ => AgentResult::Completed,
                };
                out.push(event(Some(id), None, EventKind::AgentStopped { result }));
            }
        }
        // A subagent's turn ends with `subagentStop`.
        "stop" if agent.is_none() => out.push(event(None, None, EventKind::TurnCompleted)),
        "preCompact" => out.push(event(agent.clone(), None, EventKind::Compacted)),
        _ => {}
    }
    out
}
