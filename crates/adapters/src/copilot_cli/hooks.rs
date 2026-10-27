//! Copilot CLI hook payloads → normalized events.
//!
//! AgentScope registers the camelCase events, whose payloads carry
//! `sessionId`, `timestamp` (Unix ms) and `cwd`, plus `agentId`,
//! `agentType` and `parentAgentId` inside a subagent. Tool calls, prompts
//! and subagent starts come from `events.jsonl`, which Copilot writes as they
//! happen; the hooks add what the log lacks: permission prompts, turn ends,
//! and the harness pid.
//!
//! `permissionRequest` fires for every permission check, including ones that
//! rules approve on their own, so it only records the request. The
//! `permission_prompt` notification that follows is what says a prompt is
//! actually on screen.

use std::collections::HashMap;

use agentscope_core::redact::redact_json;
use agentscope_core::{
    AgentEvent, AgentResult, Confidence, EventKind, EventSource, HarnessId, RawHookEvent,
};
use chrono::{TimeZone, Utc};
use serde_json::Value;

use crate::summary::{tool_summary, DETAIL_MAX_STRING};

/// Hook events AgentScope registers. `preToolUse` is left out on purpose:
/// Copilot denies the tool call when a `preToolUse` command fails, so a
/// missing shim (AgentScope uninstalled without disconnecting) would block
/// the agent. Every event below fails open.
pub const EVENTS: &[&str] = &[
    "sessionStart",
    "sessionEnd",
    "permissionRequest",
    "notification",
    "agentStop",
    "subagentStop",
];

/// A permission check waiting to learn whether it became a prompt.
#[derive(Debug, Clone)]
pub struct Permission {
    agent: Option<String>,
    tool: String,
    summary: String,
    detail: Option<Value>,
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

pub fn normalize(
    raw: &RawHookEvent,
    permissions: &mut HashMap<String, Permission>,
) -> Vec<AgentEvent> {
    let p = &raw.payload;
    let Some(session) = str_of(p, "sessionId").or_else(|| str_of(p, "session_id")) else {
        return Vec::new();
    };
    let at = p
        .get("timestamp")
        .and_then(Value::as_i64)
        .unwrap_or(raw.received_at as i64);
    let at = Utc
        .timestamp_millis_opt(at)
        .single()
        .unwrap_or_else(Utc::now);
    let cwd = str_of(p, "cwd").unwrap_or_default();
    let agent = str_of(p, "agentId").map(str::to_owned);

    let event = |agent: Option<String>, dedupe_key: Option<String>, kind: EventKind| AgentEvent {
        source: EventSource::Hook,
        at,
        harness: HarnessId::CopilotCli,
        session: session.to_owned(),
        agent,
        dedupe_key,
        kind,
    };

    let mut out = Vec::new();
    // Hooks inside a subagent name its parent, which the event log does not.
    if let (Some(id), Some(parent)) = (&agent, str_of(p, "parentAgentId")) {
        out.push(event(
            agent.clone(),
            Some(format!("agent-{id}")),
            EventKind::AgentStarted {
                parent: Some(parent.to_owned()),
                agent_type: str_of(p, "agentType").map(str::to_owned),
                description: None,
            },
        ));
    }

    match raw.event.as_str() {
        "sessionStart" => out.push(event(
            None,
            None,
            EventKind::SessionStarted {
                cwd: cwd.to_owned(),
                model: None,
                transcript_path: None,
                pid: None,
            },
        )),
        "sessionEnd" => {
            permissions.remove(session);
            out.push(event(None, None, EventKind::SessionEnded));
        }
        "permissionRequest" => {
            let tool = str_of(p, "toolName").unwrap_or("tool");
            let input = p
                .get("toolInput")
                .or_else(|| p.get("toolArgs"))
                .unwrap_or(&Value::Null);
            permissions.insert(
                session.to_owned(),
                Permission {
                    agent: agent.clone(),
                    tool: tool.to_owned(),
                    summary: tool_summary(tool, input, cwd),
                    detail: (!input.is_null()).then(|| redact_json(input, DETAIL_MAX_STRING)),
                },
            );
        }
        "notification" => match str_of(p, "notification_type") {
            Some("permission_prompt") => {
                let message = str_of(p, "message").unwrap_or("Permission needed");
                let request = permissions.remove(session).unwrap_or_else(|| Permission {
                    agent: agent.clone(),
                    tool: "tool".into(),
                    summary: agentscope_core::redact::truncate(
                        &agentscope_core::redact::redact_text(message),
                        crate::summary::SUMMARY_MAX_CHARS,
                    ),
                    detail: None,
                });
                out.push(event(
                    request.agent,
                    None,
                    EventKind::ApprovalRequested {
                        approval_id: format!("perm-{}", raw.received_at),
                        tool: request.tool,
                        summary: request.summary,
                        detail: request.detail,
                        confidence: Confidence::Confirmed,
                    },
                ));
            }
            Some("elicitation_dialog") => {
                out.push(event(agent.clone(), None, EventKind::WaitingForInput))
            }
            _ => {}
        },
        "agentStop" => {
            permissions.remove(session);
            out.push(event(None, None, EventKind::TurnCompleted));
        }
        "subagentStop" if agent.is_some() => {
            out.push(event(
                agent.clone(),
                None,
                EventKind::AgentStarted {
                    parent: None,
                    agent_type: str_of(p, "agentType")
                        .or_else(|| str_of(p, "agentName"))
                        .map(str::to_owned),
                    description: str_of(p, "agentDisplayName").map(str::to_owned),
                },
            ));
            out.push(event(
                agent.clone(),
                None,
                EventKind::AgentStopped {
                    result: AgentResult::Completed,
                },
            ));
        }
        _ => {}
    }
    out
}
