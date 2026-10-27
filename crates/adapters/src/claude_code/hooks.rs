//! Claude Code hook payloads → normalized events.
//!
//! Every payload carries `session_id`, `transcript_path`, `cwd` and
//! `hook_event_name`; events from inside a subagent also carry `agent_id`.

use agentscope_core::redact::redact_json;
use agentscope_core::{
    AgentEvent, AgentResult, Confidence, EventKind, EventSource, HarnessId, RawHookEvent,
    Resolution,
};
use chrono::{TimeZone, Utc};
use serde_json::Value;

use crate::summary::{tool_summary, DETAIL_MAX_STRING};

/// Hook events AgentScope registers. All are observe-only: the shim never
/// prints anything, so none of them can change what Claude Code does.
pub const EVENTS: &[&str] = &[
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PermissionDenied",
    "PostToolUse",
    "PostToolUseFailure",
    "Notification",
    "SubagentStart",
    "SubagentStop",
    "Stop",
    "StopFailure",
    "PreCompact",
];

/// Tools that stop and wait for the user to answer a question.
const ASKS_USER: &[&str] = &["AskUserQuestion"];

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

pub fn normalize(raw: &RawHookEvent) -> Vec<AgentEvent> {
    let p = &raw.payload;
    // Copilot CLI also runs Claude-style hooks but sends camelCase payloads
    // without `hook_event_name`; those are not ours to interpret.
    let (Some(session), Some(name)) = (str_of(p, "session_id"), str_of(p, "hook_event_name"))
    else {
        return Vec::new();
    };
    let at = Utc
        .timestamp_millis_opt(raw.received_at as i64)
        .single()
        .unwrap_or_else(Utc::now);
    let cwd = str_of(p, "cwd").unwrap_or_default();
    let agent = str_of(p, "agent_id").map(str::to_owned);
    let tool = str_of(p, "tool_name").unwrap_or("tool");
    let tool_use_id = str_of(p, "tool_use_id").map(str::to_owned);
    let input = p.get("tool_input").unwrap_or(&Value::Null);

    let event = |dedupe_key: Option<String>, kind: EventKind| AgentEvent {
        source: EventSource::Hook,
        at,
        harness: HarnessId::ClaudeCode,
        session: session.to_owned(),
        agent: agent.clone(),
        dedupe_key,
        kind,
    };

    let kinds: Vec<(Option<String>, EventKind)> = match name {
        "SessionStart" => vec![(
            None,
            EventKind::SessionStarted {
                cwd: cwd.to_owned(),
                model: model_name(p),
                transcript_path: str_of(p, "transcript_path").map(str::to_owned),
                // The engine resolves the harness pid from `raw.ppid`: the shim
                // may run under a shell that exits right after.
                pid: None,
            },
        )],
        "SessionEnd" => vec![(None, EventKind::SessionEnded)],
        "UserPromptSubmit" => vec![(
            str_of(p, "prompt_id").map(str::to_owned),
            EventKind::PromptSubmitted {
                prompt: str_of(p, "prompt").unwrap_or_default().to_owned(),
            },
        )],
        "PreToolUse" => {
            let mut kinds = vec![(
                tool_use_id,
                EventKind::ToolStarted {
                    tool: tool.to_owned(),
                    summary: tool_summary(tool, input, cwd),
                },
            )];
            if ASKS_USER.contains(&tool) {
                kinds.push((None, EventKind::WaitingForInput));
            }
            kinds
        }
        "PermissionRequest" => vec![(
            None,
            EventKind::ApprovalRequested {
                // The payload has no tool_use_id; one approval per prompt is
                // enough since a prompt blocks the agent until answered.
                approval_id: tool_use_id.unwrap_or_else(|| format!("perm-{}", raw.received_at)),
                tool: tool.to_owned(),
                summary: tool_summary(tool, input, cwd),
                detail: (!input.is_null()).then(|| redact_json(input, DETAIL_MAX_STRING)),
                confidence: Confidence::Confirmed,
            },
        )],
        "PermissionDenied" => vec![(
            None,
            EventKind::ApprovalResolved {
                approval_id: None,
                resolution: Resolution::Denied,
            },
        )],
        "PostToolUse" => vec![(tool_use_id, EventKind::ToolFinished)],
        "PostToolUseFailure" => vec![(
            tool_use_id,
            EventKind::ToolFailed {
                error: str_of(p, "error").map(|e| agentscope_core::redact::truncate(e, 300)),
            },
        )],
        "Notification" => match str_of(p, "notification_type") {
            // `PermissionRequest` already reported this prompt, with details.
            Some("permission_prompt") | None => Vec::new(),
            Some(_) => vec![(None, EventKind::WaitingForInput)],
        },
        "SubagentStart" => vec![(
            None,
            EventKind::AgentStarted {
                parent: None,
                agent_type: str_of(p, "agent_type").map(str::to_owned),
                description: None,
            },
        )],
        "SubagentStop" => vec![(
            None,
            EventKind::AgentStopped {
                result: AgentResult::Completed,
            },
        )],
        "Stop" | "StopFailure" => vec![(None, EventKind::TurnCompleted)],
        "PreCompact" => vec![(None, EventKind::Compacted)],
        _ => Vec::new(),
    };
    kinds
        .into_iter()
        .map(|(key, kind)| event(key, kind))
        .collect()
}

/// `model` is a string in current versions; older ones sent `{ id, display_name }`.
fn model_name(p: &Value) -> Option<String> {
    match p.get("model")? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Object(m) => m
            .get("id")
            .or_else(|| m.get("display_name"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        _ => None,
    }
}
