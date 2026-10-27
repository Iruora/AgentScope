//! Cursor (the desktop agent and the `cursor-agent` CLI): hooks in
//! `~/.cursor/hooks.json`. Cursor has no event for "a prompt is showing", so
//! shell and MCP calls that get no result for a few seconds become inferred
//! approvals. Notes on payload shapes are in `docs/adapters/cursor.md`.

mod hooks;
mod install;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use agentscope_core::{AgentEvent, HarnessId, RawHookEvent};

use crate::*;

pub struct Cursor {
    home: Option<PathBuf>,
    /// Subagents seen in `subagentStart`, so their own events and their stop
    /// land on the right session.
    subagents: Mutex<HashMap<String, hooks::Subagent>>,
}

impl Cursor {
    /// Uses `~/.cursor`.
    pub fn new() -> Self {
        Self::from_home(home_dir(".cursor"))
    }

    pub fn with_home(dir: impl Into<PathBuf>) -> Self {
        Self::from_home(Some(dir.into()))
    }

    fn from_home(home: Option<PathBuf>) -> Self {
        Self {
            home,
            subagents: Mutex::new(HashMap::new()),
        }
    }

    fn hooks_path(&self) -> Result<PathBuf> {
        self.home
            .as_ref()
            .map(|d| d.join("hooks.json"))
            .ok_or_else(|| AdapterError::NotFound("the Cursor config directory".into()))
    }
}

impl Default for Cursor {
    fn default() -> Self {
        Self::new()
    }
}

impl Adapter for Cursor {
    fn id(&self) -> HarnessId {
        HarnessId::Cursor
    }

    fn detect(&self) -> Detection {
        detect_config_dir(self.id(), self.home.clone())
    }

    fn hooks_status(&self, shim_path: &Path) -> HookStatus {
        match self.hooks_path() {
            Ok(path) => install::status(&path, shim_path),
            Err(_) => HookStatus::NotInstalled,
        }
    }

    fn install_hooks(&self, plan: &HookPlan) -> Result<Diff> {
        install::install(&self.hooks_path()?, plan)
    }

    fn uninstall_hooks(&self, dry_run: bool) -> Result<Diff> {
        install::uninstall(&self.hooks_path()?, dry_run)
    }

    fn normalize_hook(&self, raw: &RawHookEvent) -> Vec<AgentEvent> {
        let mut subagents = self.subagents.lock().unwrap_or_else(|e| e.into_inner());
        hooks::normalize(raw, &mut subagents)
    }

    /// Cursor's agent transcripts carry no timestamps or ids, so they cannot
    /// be merged with hook events; Cursor is followed through hooks only.
    fn transcript_roots(&self) -> Vec<WatchRoot> {
        Vec::new()
    }

    fn parse_transcript_chunk(&self, _file: &TranscriptFile, _bytes: &[u8]) -> Vec<AgentEvent> {
        Vec::new()
    }

    /// The desktop app (`Cursor`, `cursor`) or the CLI, which runs as
    /// `cursor-agent` or `node …/cursor-agent/…`.
    fn match_process(&self, p: &ProcessInfo) -> Option<ProcessMatch> {
        match_by_name(self.id(), &["cursor", "Cursor", "cursor-agent", "agent"], p)
            .filter(|_| {
                // `agent` alone is too generic unless it is Cursor's CLI.
                Path::new(&p.name).file_stem().is_some_and(|s| s != "agent")
                    || p.cmd.iter().any(|a| a.contains("cursor"))
            })
            .or_else(|| {
                let exe = Path::new(&p.name).file_stem()?.to_str()?;
                let runs_cli = exe == "node"
                    && p.cmd
                        .iter()
                        .skip(1)
                        .take(2)
                        .any(|a| a.contains("cursor-agent"));
                runs_cli.then(|| ProcessMatch {
                    harness: self.id(),
                    session: None,
                    cwd: p.cwd.clone(),
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(name: &str, cmd: &[&str]) -> ProcessInfo {
        ProcessInfo {
            pid: 1,
            ppid: None,
            name: name.into(),
            cmd: cmd.iter().map(|s| s.to_string()).collect(),
            cwd: None,
        }
    }

    #[test]
    fn matches_the_app_and_the_cli() {
        let c = Cursor::with_home("/nope");
        assert!(c.match_process(&process("Cursor", &[])).is_some());
        assert!(c.match_process(&process("cursor-agent", &[])).is_some());
        assert!(c
            .match_process(&process(
                "node",
                &[
                    "node",
                    "/home/u/.local/share/cursor-agent/versions/1/index.js"
                ]
            ))
            .is_some());
        assert!(c
            .match_process(&process("agent", &["/home/u/.local/bin/agent"]))
            .is_none());
        assert!(c
            .match_process(&process(
                "agent",
                &["/home/u/.local/share/cursor-agent/agent"]
            ))
            .is_some());
        assert!(c
            .match_process(&process("node", &["node", "app.js"]))
            .is_none());
    }
}
