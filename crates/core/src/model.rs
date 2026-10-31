//! Entities shown in the UI. Mirrors section 4.1 of `docs/SPEC.md`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::HarnessId;

pub type Timestamp = DateTime<Utc>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    /// `${harness}:${nativeSessionId}`
    pub id: String,
    pub harness: HarnessId,
    pub native_id: String,
    pub title: Option<String>,
    pub cwd: String,
    pub repo: Option<Repo>,
    pub model: Option<String>,
    pub started_at: Timestamp,
    pub last_event_at: Timestamp,
    pub ended_at: Option<Timestamp>,
    pub state: AgentState,
    pub pid: Option<u32>,
    pub terminal: Option<TerminalRef>,
    pub transcript_path: Option<String>,
    pub tokens: Option<Tokens>,
    pub root_agent_id: String,
    /// Files the session's agents wrote, most recent first, capped at
    /// [`Session::MAX_FILES`].
    #[serde(default)]
    pub files: Vec<FileTouch>,
}

impl Session {
    pub const MAX_FILES: usize = 300;

    pub fn key(harness: HarnessId, native_id: &str) -> String {
        format!("{harness}:{native_id}")
    }
}

/// A file an agent edited, created or deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileTouch {
    /// Absolute path.
    pub path: String,
    pub edits: u32,
    pub first_at: Timestamp,
    pub last_at: Timestamp,
    /// The agent that touched it last.
    pub agent_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Repo {
    pub root: String,
    pub branch: Option<String>,
}

/// Where the agent runs, for "Go to agent". Filled in by step 3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TerminalRef {
    /// The harness process, usually the shim's parent.
    pub pid: u32,
    /// The ancestor that owns the window (terminal app or IDE), once resolved.
    pub window_pid: Option<u32>,
    pub app: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    // Exported as `number`: exact up to 2^53, far beyond any session.
    #[specta(type = f64)]
    pub input: u64,
    #[specta(type = f64)]
    pub output: u64,
    #[specta(type = Option<f64>)]
    pub cached: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    /// `${sessionId}/${nativeAgentId|"main"}`
    pub id: String,
    pub session_id: String,
    /// `None` for the main agent. Subagents may nest more than one level.
    pub parent_agent_id: Option<String>,
    pub kind: AgentKind,
    pub agent_type: Option<String>,
    pub description: Option<String>,
    pub state: AgentState,
    pub current_activity: Option<Activity>,
    pub started_at: Timestamp,
    pub ended_at: Option<Timestamp>,
    pub result: Option<AgentResult>,
    /// The same tool call failing again and again.
    #[serde(default)]
    pub failures: Option<FailureStreak>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FailureStreak {
    pub summary: String,
    pub count: u32,
    pub last_at: Timestamp,
}

impl Agent {
    pub const MAIN: &'static str = "main";

    pub fn key(session_id: &str, native_agent_id: &str) -> String {
        format!("{session_id}/{native_agent_id}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    Main,
    Subagent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentResult {
    Completed,
    Failed,
    Aborted,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    Starting,
    Thinking,
    RunningTool,
    /// A confirmed permission prompt.
    WaitingApproval,
    /// An inferred permission prompt (Cursor), shown softer.
    MaybeWaiting,
    /// The agent finished a turn or asked the user something.
    WaitingInput,
    Idle,
    Ended,
    Errored,
    #[default]
    Unknown,
}

impl AgentState {
    /// States that pin a session to the "Needs you" strip.
    pub fn needs_user(self) -> bool {
        matches!(
            self,
            AgentState::WaitingApproval | AgentState::MaybeWaiting | AgentState::WaitingInput
        )
    }

    pub fn is_live(self) -> bool {
        !matches!(self, AgentState::Ended | AgentState::Errored)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub kind: ActivityKind,
    pub tool: Option<String>,
    /// "Bash: npm test", "Edit src/app.ts"
    pub summary: String,
    pub started_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    Tool,
    Message,
    Compact,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PendingApproval {
    pub id: String,
    pub session_id: String,
    pub agent_id: String,
    pub harness: HarnessId,
    pub tool: String,
    /// Redacted and truncated.
    pub summary: String,
    /// Full tool input, shown on expand.
    #[specta(type = Option<specta_typescript::Unknown>)]
    pub detail: Option<serde_json::Value>,
    pub requested_at: Timestamp,
    pub confidence: Confidence,
    pub resolved_at: Option<Timestamp>,
    pub resolution: Option<Resolution>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Confirmed,
    Inferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    Approved,
    Denied,
    Cancelled,
    Unknown,
}

/// Something about a live session worth a look, worked out from the graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Concern {
    /// Stable while the concern lasts, to notify once.
    pub id: String,
    pub kind: ConcernKind,
    pub session_id: String,
    pub agent_id: Option<String>,
    /// "Bash: npm test has run for 14 min"
    pub message: String,
    pub since: Timestamp,
    /// For conflicts: the file and the other sessions writing it.
    pub path: Option<String>,
    pub other_session_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ConcernKind {
    /// One tool call has run for a long time.
    LongTool,
    /// The same tool call failed several times in a row.
    RepeatedFailure,
    /// A working session has gone quiet.
    Silent,
    /// Another live session wrote the same file recently.
    Conflict,
}
