//! One adapter per harness. Adapters are pure translators: the core owns IO
//! scheduling, dedupe, and state. See section 3.4 of `docs/SPEC.md`.
//!
//! The adapters in this scaffold only detect installs and match processes;
//! hook install and payload translation land per harness in steps 3 and 4.

use std::path::{Path, PathBuf};

use agentscope_core::{AgentEvent, HarnessId, RawHookEvent, Timestamp};
use serde::Serialize;
use specta::Type;

mod claude_code;
mod codex;
mod copilot_cli;
mod cursor;

pub use claude_code::ClaudeCode;
pub use codex::Codex;
pub use copilot_cli::CopilotCli;
pub use cursor::Cursor;

pub trait Adapter: Send + Sync {
    fn id(&self) -> HarnessId;
    /// Find the install, config dirs, version. Cheap; runs at startup and on demand.
    fn detect(&self) -> Detection;
    fn hooks_status(&self) -> HookStatus;
    fn install_hooks(&self, plan: &HookPlan) -> Result<Diff>;
    fn uninstall_hooks(&self) -> Result<Diff>;
    /// Turn a raw hook payload into zero or more normalized events.
    fn normalize_hook(&self, raw: &RawHookEvent) -> Vec<AgentEvent>;
    /// Paths to watch for transcripts; the core owns watching and tailing.
    fn transcript_roots(&self) -> Vec<WatchRoot>;
    fn parse_transcript_chunk(&self, file: &TranscriptFile, bytes: &[u8]) -> Vec<AgentEvent>;
    /// Optional periodic scan (SQLite indexes, lock files).
    fn scan(&self, _since: Timestamp) -> Vec<AgentEvent> {
        Vec::new()
    }
    fn match_process(&self, p: &ProcessInfo) -> Option<ProcessMatch>;
}

/// Every adapter, in display order.
pub fn all() -> Vec<Box<dyn Adapter>> {
    vec![
        Box::new(ClaudeCode),
        Box::new(Codex),
        Box::new(CopilotCli),
        Box::new(Cursor),
    ]
}

pub fn for_harness(id: HarnessId) -> Box<dyn Adapter> {
    match id {
        HarnessId::ClaudeCode => Box::new(ClaudeCode),
        HarnessId::Codex => Box::new(Codex),
        HarnessId::CopilotCli => Box::new(CopilotCli),
        HarnessId::Cursor => Box::new(Cursor),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("{0} is not supported by this adapter yet")]
    NotImplemented(&'static str),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, AdapterError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Detection {
    pub harness: HarnessId,
    pub installed: bool,
    /// The user-level config dir, if it exists.
    pub config_dir: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum HookStatus {
    NotInstalled,
    Installed,
    /// Written, but the harness has to trust it first (Codex `/hooks`).
    NeedsTrust,
    Outdated,
}

/// What to register. Observe mode only registers non-blocking events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookPlan {
    pub shim_path: PathBuf,
    pub events: Vec<String>,
}

/// A config change shown to the user before it is written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Diff {
    pub path: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchRoot {
    pub path: PathBuf,
    pub recursive: bool,
    /// File name suffix to tail, e.g. `.jsonl`.
    pub suffix: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptFile {
    pub path: PathBuf,
    /// Byte offset of the chunk within the file.
    pub offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub ppid: Option<u32>,
    pub name: String,
    pub cmd: Vec<String>,
    pub cwd: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessMatch {
    pub harness: HarnessId,
    /// The native session id, when the process reveals it.
    pub session: Option<String>,
    pub cwd: Option<PathBuf>,
}

/// Shared detection: an adapter counts as installed when its config dir exists.
pub(crate) fn detect_config_dir(harness: HarnessId, dir: Option<PathBuf>) -> Detection {
    let config_dir = dir.filter(|d| d.is_dir());
    Detection {
        harness,
        installed: config_dir.is_some(),
        config_dir: config_dir.map(|d| d.display().to_string()),
        version: None,
    }
}

pub(crate) fn home_dir(child: &str) -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(child))
}

/// Matches a process whose executable name is one of `names`
/// (ignoring a `.exe` suffix on Windows).
pub(crate) fn match_by_name(
    harness: HarnessId,
    names: &[&str],
    p: &ProcessInfo,
) -> Option<ProcessMatch> {
    let exe = Path::new(&p.name).file_stem()?.to_str()?;
    names.contains(&exe).then(|| ProcessMatch {
        harness,
        session: None,
        cwd: p.cwd.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(name: &str) -> ProcessInfo {
        ProcessInfo {
            pid: 1,
            ppid: None,
            name: name.into(),
            cmd: vec![],
            cwd: None,
        }
    }

    #[test]
    fn registry_covers_every_harness_once() {
        let ids: Vec<HarnessId> = all().iter().map(|a| a.id()).collect();
        assert_eq!(ids, HarnessId::ALL);
        for id in HarnessId::ALL {
            assert_eq!(for_harness(id).id(), id);
        }
    }

    #[test]
    fn matches_processes_by_executable_name() {
        assert!(ClaudeCode.match_process(&process("claude")).is_some());
        assert!(Codex.match_process(&process("codex.exe")).is_some());
        assert!(CopilotCli.match_process(&process("copilot")).is_some());
        assert!(Cursor.match_process(&process("cursor-agent")).is_some());
        assert!(ClaudeCode
            .match_process(&process("claude-helper"))
            .is_none());
    }
}
