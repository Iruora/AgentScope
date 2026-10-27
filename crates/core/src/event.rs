//! Normalized events. Every source (hooks, transcripts, scans, the process
//! table) is translated into these by an adapter; one reducer applies them.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::model::{AgentResult, Confidence, Resolution, Timestamp, Tokens};
use crate::HarnessId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    Hook,
    Transcript,
    Scan,
    Process,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentEvent {
    pub source: EventSource,
    /// The harness's own timestamp when it has one, else when we received it.
    pub at: Timestamp,
    pub harness: HarnessId,
    /// The harness's native session id.
    pub session: String,
    /// The harness's native agent id. `None` means the session's main agent.
    pub agent: Option<String>,
    /// `tool_use_id`, `toolCallId`, or an event uuid. Used to merge the same
    /// event seen through a hook and a transcript.
    pub dedupe_key: Option<String>,
    pub kind: EventKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum EventKind {
    SessionStarted {
        cwd: String,
        model: Option<String>,
        transcript_path: Option<String>,
        pid: Option<u32>,
    },
    SessionEnded,
    /// Details learned after the start. `None` fields are left as they are.
    SessionInfo {
        cwd: Option<String>,
        model: Option<String>,
        /// A harness-provided name, preferred over the first prompt.
        title: Option<String>,
    },
    PromptSubmitted {
        prompt: String,
    },
    AgentStarted {
        /// Native id of the spawning agent; `None` means the main agent.
        parent: Option<String>,
        agent_type: Option<String>,
        description: Option<String>,
    },
    AgentStopped {
        result: AgentResult,
    },
    ToolStarted {
        tool: String,
        summary: String,
    },
    ToolFinished,
    ToolFailed {
        error: Option<String>,
    },
    ApprovalRequested {
        approval_id: String,
        tool: String,
        summary: String,
        #[specta(type = Option<specta_typescript::Unknown>)]
        detail: Option<serde_json::Value>,
        confidence: Confidence,
    },
    ApprovalResolved {
        /// `None` resolves every open approval of the agent.
        approval_id: Option<String>,
        resolution: Resolution,
    },
    WaitingForInput,
    TurnCompleted,
    Compacted,
    /// Running totals for the session, replacing earlier ones.
    TokensReported {
        tokens: Tokens,
    },
    /// Usage of one model response, added to the session's totals.
    TokensUsed {
        tokens: Tokens,
    },
    Heartbeat {
        pid: u32,
    },
    Unknown {
        event: String,
        #[specta(type = specta_typescript::Unknown)]
        raw: serde_json::Value,
    },
}

impl EventKind {
    pub fn name(&self) -> &'static str {
        match self {
            EventKind::SessionStarted { .. } => "sessionStarted",
            EventKind::SessionEnded => "sessionEnded",
            EventKind::SessionInfo { .. } => "sessionInfo",
            EventKind::PromptSubmitted { .. } => "promptSubmitted",
            EventKind::AgentStarted { .. } => "agentStarted",
            EventKind::AgentStopped { .. } => "agentStopped",
            EventKind::ToolStarted { .. } => "toolStarted",
            EventKind::ToolFinished => "toolFinished",
            EventKind::ToolFailed { .. } => "toolFailed",
            EventKind::ApprovalRequested { .. } => "approvalRequested",
            EventKind::ApprovalResolved { .. } => "approvalResolved",
            EventKind::WaitingForInput => "waitingForInput",
            EventKind::TurnCompleted => "turnCompleted",
            EventKind::Compacted => "compacted",
            EventKind::TokensReported { .. } => "tokensReported",
            EventKind::TokensUsed { .. } => "tokensUsed",
            EventKind::Heartbeat { .. } => "heartbeat",
            EventKind::Unknown { .. } => "unknown",
        }
    }

    /// Events that show the agent doing something. They reopen a session
    /// that was marked ended earlier, e.g. after `claude --resume`.
    pub fn is_activity(&self) -> bool {
        matches!(
            self,
            EventKind::SessionStarted { .. }
                | EventKind::PromptSubmitted { .. }
                | EventKind::AgentStarted { .. }
                | EventKind::ToolStarted { .. }
                | EventKind::ApprovalRequested { .. }
                // Only sent for a process that is running right now.
                | EventKind::Heartbeat { .. }
        )
    }
}
