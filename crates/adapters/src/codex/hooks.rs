//! Codex hook payloads → normalized events.
//!
//! Codex follows Claude Code's hook contract: every payload carries
//! `session_id`, `cwd`, `hook_event_name` and `model`; turn-scoped ones add
//! `turn_id`, and events from inside a subagent add `agent_id` (the
//! subagent's thread id) while `session_id` stays the root session.

use agentscope_core::redact::redact_json;
use agentscope_core::{
    AgentEvent, AgentResult, Confidence, EventKind, EventSource, HarnessId, RawHookEvent,
};
use chrono::{TimeZone, Utc};
use serde_json::Value;

use crate::summary::{tool_files, tool_summary, DETAIL_MAX_STRING};

/// Hook events AgentScope registers. All are observe-only.
pub const EVENTS: &[&str] = &[
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "SubagentStart",
    "SubagentStop",
    "Stop",
    "Interrupt",
    "PreCompact",
];

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

pub fn normalize(raw: &RawHookEvent) -> Vec<AgentEvent> {
    let p = &raw.payload;
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
    let turn = str_of(p, "turn_id").map(str::to_owned);
    let tool = str_of(p, "tool_name").unwrap_or("tool");
    let tool_use_id = str_of(p, "tool_use_id").map(str::to_owned);
    let input = p.get("tool_input").unwrap_or(&Value::Null);

    let event = |dedupe_key: Option<String>, kind: EventKind| AgentEvent {
        source: EventSource::Hook,
        at,
        harness: HarnessId::Codex,
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
                model: str_of(p, "model").map(str::to_owned),
                transcript_path: str_of(p, "transcript_path").map(str::to_owned),
                pid: None,
            },
        )],
        "SessionEnd" => vec![(None, EventKind::SessionEnded)],
        // The rollout's `user_message` carries the same turn id.
        "UserPromptSubmit" => vec![(
            turn,
            EventKind::PromptSubmitted {
                prompt: str_of(p, "prompt").unwrap_or_default().to_owned(),
            },
        )],
        "PreToolUse" => vec![(
            tool_use_id,
            EventKind::ToolStarted {
                tool: tool.to_owned(),
                summary: tool_summary(tool, input, cwd),
                files: tool_files(tool, input, cwd),
            },
        )],
        // Only fires for calls that need the user, so it is a real prompt.
        "PermissionRequest" => vec![(
            None,
            EventKind::ApprovalRequested {
                approval_id: format!(
                    "perm-{}-{}",
                    turn.as_deref().unwrap_or("turn"),
                    raw.received_at
                ),
                tool: tool.to_owned(),
                summary: tool_summary(tool, input, cwd),
                detail: (!input.is_null()).then(|| redact_json(input, DETAIL_MAX_STRING)),
                confidence: Confidence::Confirmed,
            },
        )],
        "PostToolUse" => vec![(tool_use_id, EventKind::ToolFinished)],
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
        "Stop" => vec![(
            turn.map(|t| format!("turn-end-{t}")),
            EventKind::TurnCompleted,
        )],
        "Interrupt" => vec![(None, EventKind::TurnCompleted)],
        "PreCompact" => vec![(None, EventKind::Compacted)],
        _ => Vec::new(),
    };
    kinds
        .into_iter()
        .map(|(key, kind)| event(key, kind))
        .collect()
}
