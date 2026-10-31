//! Wire format between the `agentscope-hook` shim and the AgentScope app.
//!
//! The shim must stay tiny, so this crate only depends on `serde` and `dirs`.

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Bumped when [`RawHookEvent`] or [`IngestInfo`] change incompatibly.
pub const PROTOCOL_VERSION: u32 = 1;

/// HTTP path the shim posts wrapped events to.
pub const INGEST_PATH: &str = "/v1/hook";

/// HTTP path the shim posts a permission prompt to when it waits for the
/// user's answer from AgentScope (`--decide`). The reply is a [`DecideReply`].
pub const DECIDE_PATH: &str = "/v1/decide";

/// The longest the app may hold a `--decide` request. The user's chosen
/// window is shorter; this is the shim's own cap in case the app hangs.
pub const DECIDE_MAX_WAIT_SECS: u64 = 130;

/// Header carrying the per-launch token from `ingest.json`.
pub const TOKEN_HEADER: &str = "x-agentscope-token";

/// Upper bound for the spool the shim writes while the app is not running.
pub const SPOOL_MAX_BYTES: u64 = 5 * 1024 * 1024;

/// Upper bound for a single hook payload. Larger payloads are truncated by the shim.
pub const MAX_PAYLOAD_BYTES: usize = 1024 * 1024;

/// Env var that overrides the data directory (tests, portable installs).
pub const DATA_DIR_ENV: &str = "AGENTSCOPE_DATA_DIR";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum HarnessId {
    ClaudeCode,
    Codex,
    CopilotCli,
    Cursor,
}

impl HarnessId {
    pub const ALL: [HarnessId; 4] = [
        HarnessId::ClaudeCode,
        HarnessId::Codex,
        HarnessId::CopilotCli,
        HarnessId::Cursor,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            HarnessId::ClaudeCode => "claude-code",
            HarnessId::Codex => "codex",
            HarnessId::CopilotCli => "copilot-cli",
            HarnessId::Cursor => "cursor",
        }
    }
}

impl fmt::Display for HarnessId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownHarness(pub String);

impl fmt::Display for UnknownHarness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown harness `{}`", self.0)
    }
}

impl std::error::Error for UnknownHarness {}

impl FromStr for HarnessId {
    type Err = UnknownHarness;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        HarnessId::ALL
            .into_iter()
            .find(|h| h.as_str() == s)
            .ok_or_else(|| UnknownHarness(s.to_owned()))
    }
}

/// A hook payload as received from a harness, wrapped with what the shim knows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawHookEvent {
    pub v: u32,
    pub harness: HarnessId,
    /// The harness's own event name, e.g. `PreToolUse` or `preToolUse`.
    pub event: String,
    /// Unix time in milliseconds when the shim (or ingest server) received the payload.
    pub received_at: u64,
    /// The shim's pid. Its parent is the harness process.
    pub pid: Option<u32>,
    /// The harness process pid, used by "Go to agent" to find the terminal.
    pub ppid: Option<u32>,
    /// The JSON the harness wrote to stdin, untouched. Non-JSON input is kept as a string.
    pub payload: serde_json::Value,
}

/// The user's answer to a permission prompt, given in AgentScope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "behavior", rename_all = "lowercase")]
pub enum Decision {
    Allow,
    Deny { message: String },
}

/// The app's reply on [`DECIDE_PATH`]. `None` means "no answer here": the
/// harness should ask in its own prompt, as if AgentScope were not there.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DecideReply {
    pub decision: Option<Decision>,
}

/// Written by the app to `<data dir>/ingest.json` on every start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestInfo {
    pub v: u32,
    pub port: u16,
    pub token: String,
    /// The app's pid, so a stale file from a crashed app can be recognized.
    pub pid: u32,
}

/// AgentScope's data directory: `$AGENTSCOPE_DATA_DIR`, else the OS data dir + `AgentScope`.
pub fn data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(DATA_DIR_ENV).filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    dirs::data_dir().map(|d| d.join("AgentScope"))
}

pub fn ingest_info_path(data_dir: &Path) -> PathBuf {
    data_dir.join("ingest.json")
}

pub fn spool_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("spool")
}

/// Current Unix time in milliseconds.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_ids_round_trip() {
        for h in HarnessId::ALL {
            assert_eq!(h.as_str().parse::<HarnessId>().unwrap(), h);
            let json = serde_json::to_string(&h).unwrap();
            assert_eq!(json, format!("\"{}\"", h.as_str()));
        }
        assert!("claude".parse::<HarnessId>().is_err());
    }

    #[test]
    fn decisions_use_the_claude_code_shape() {
        let deny = Decision::Deny {
            message: "No".into(),
        };
        assert_eq!(
            serde_json::to_value(&deny).unwrap(),
            serde_json::json!({ "behavior": "deny", "message": "No" })
        );
        assert_eq!(
            serde_json::to_value(Decision::Allow).unwrap(),
            serde_json::json!({ "behavior": "allow" })
        );
        let none: DecideReply = serde_json::from_str(r#"{"decision":null}"#).unwrap();
        assert_eq!(none.decision, None);
    }

    #[test]
    fn raw_event_round_trips() {
        let raw = RawHookEvent {
            v: PROTOCOL_VERSION,
            harness: HarnessId::Codex,
            event: "PreToolUse".into(),
            received_at: 1,
            pid: Some(2),
            ppid: Some(3),
            payload: serde_json::json!({ "session_id": "abc" }),
        };
        let back: RawHookEvent =
            serde_json::from_str(&serde_json::to_string(&raw).unwrap()).unwrap();
        assert_eq!(back, raw);
    }
}
