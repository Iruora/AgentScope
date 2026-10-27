//! Codex CLI: hooks in `~/.codex/hooks.json`, trusted through `/hooks` and
//! recorded in `~/.codex/config.toml`; rollouts under
//! `~/.codex/sessions/YYYY/MM/DD/`. Notes on payload shapes are in
//! `docs/adapters/codex.md`.

mod hooks;
mod install;
mod rollout;

use std::path::{Path, PathBuf};

use agentscope_core::{AgentEvent, HarnessId, RawHookEvent};

use crate::*;

/// Env var Codex reads to move its home away from `~/.codex`.
const HOME_ENV: &str = "CODEX_HOME";

pub struct Codex {
    home: Option<PathBuf>,
}

impl Codex {
    /// Uses `$CODEX_HOME`, else `~/.codex`.
    pub fn new() -> Self {
        let home = std::env::var_os(HOME_ENV)
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .or_else(|| home_dir(".codex"));
        Self { home }
    }

    pub fn with_home(dir: impl Into<PathBuf>) -> Self {
        Self {
            home: Some(dir.into()),
        }
    }

    fn home(&self) -> Result<&Path> {
        self.home
            .as_deref()
            .ok_or_else(|| AdapterError::NotFound("the Codex home directory".into()))
    }
}

impl Default for Codex {
    fn default() -> Self {
        Self::new()
    }
}

impl Adapter for Codex {
    fn id(&self) -> HarnessId {
        HarnessId::Codex
    }

    fn detect(&self) -> Detection {
        detect_config_dir(self.id(), self.home.clone())
    }

    fn hooks_status(&self, shim_path: &Path) -> HookStatus {
        match self.home() {
            Ok(home) => install::status(home, shim_path),
            Err(_) => HookStatus::NotInstalled,
        }
    }

    fn install_hooks(&self, plan: &HookPlan) -> Result<Diff> {
        install::install(self.home()?, plan)
    }

    fn uninstall_hooks(&self, dry_run: bool) -> Result<Diff> {
        install::uninstall(self.home()?, dry_run)
    }

    fn normalize_hook(&self, raw: &RawHookEvent) -> Vec<AgentEvent> {
        hooks::normalize(raw)
    }

    fn transcript_roots(&self) -> Vec<WatchRoot> {
        self.home
            .iter()
            .map(|d| WatchRoot {
                path: d.join("sessions"),
                recursive: true,
                // Cold rollouts are recompressed to `.jsonl.zst`; by then they
                // have been read already.
                suffix: ".jsonl",
            })
            .collect()
    }

    fn parse_transcript_chunk(&self, file: &TranscriptFile, bytes: &[u8]) -> Vec<AgentEvent> {
        rollout::parse(file, bytes)
    }

    fn match_process(&self, p: &ProcessInfo) -> Option<ProcessMatch> {
        // npm installs run `node …/@openai/codex/bin/codex.js`, which spawns
        // the native binary; matching the binary is enough.
        match_by_name(self.id(), &["codex"], p)
    }
}
