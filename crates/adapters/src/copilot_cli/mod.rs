//! GitHub Copilot CLI: hooks in a dedicated `~/.copilot/hooks/agentscope.json`,
//! the live event log in `~/.copilot/session-state/<id>/events.jsonl`, and
//! `inuse.<pid>.lock` files for liveness. Notes on payload shapes are in
//! `docs/adapters/copilot-cli.md`.

mod events;
mod hooks;
mod install;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use agentscope_core::{AgentEvent, EventKind, EventSource, HarnessId, RawHookEvent, Timestamp};
use chrono::Utc;

use crate::*;

/// Env var Copilot CLI reads to move its home away from `~/.copilot`.
const HOME_ENV: &str = "COPILOT_HOME";

pub struct CopilotCli {
    home: Option<PathBuf>,
    /// The latest `permissionRequest` per session, which the matching
    /// `permission_prompt` notification turns into an approval.
    permissions: Mutex<HashMap<String, hooks::Permission>>,
}

impl CopilotCli {
    /// Uses `$COPILOT_HOME`, else `~/.copilot`.
    pub fn new() -> Self {
        let home = std::env::var_os(HOME_ENV)
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .or_else(|| home_dir(".copilot"));
        Self::from_home(home)
    }

    pub fn with_home(dir: impl Into<PathBuf>) -> Self {
        Self::from_home(Some(dir.into()))
    }

    fn from_home(home: Option<PathBuf>) -> Self {
        Self {
            home,
            permissions: Mutex::new(HashMap::new()),
        }
    }

    fn hooks_path(&self) -> Result<PathBuf> {
        self.home
            .as_ref()
            .map(|d| d.join("hooks").join(install::FILE_NAME))
            .ok_or_else(|| AdapterError::NotFound("the Copilot CLI home directory".into()))
    }
}

impl Default for CopilotCli {
    fn default() -> Self {
        Self::new()
    }
}

impl Adapter for CopilotCli {
    fn id(&self) -> HarnessId {
        HarnessId::CopilotCli
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
        let mut permissions = self.permissions.lock().unwrap_or_else(|e| e.into_inner());
        hooks::normalize(raw, &mut permissions)
    }

    fn transcript_roots(&self) -> Vec<WatchRoot> {
        self.home
            .iter()
            .map(|d| WatchRoot {
                path: d.join("session-state"),
                recursive: true,
                suffix: "events.jsonl",
            })
            .collect()
    }

    fn parse_transcript_chunk(&self, file: &TranscriptFile, bytes: &[u8]) -> Vec<AgentEvent> {
        events::parse(file, bytes)
    }

    /// A running session holds `inuse.<pid>.lock` in its folder, and
    /// `workspace.yaml` has its cwd and name. The engine drops heartbeats
    /// whose process is gone.
    fn scan(&self, _since: Timestamp) -> Vec<AgentEvent> {
        let Some(dir) = self.home.as_ref().map(|d| d.join("session-state")) else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let now = Utc::now();
        let mut out = Vec::new();
        for session_dir in entries.flatten().map(|e| e.path()) {
            let Some(session) = session_dir.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Ok(files) = std::fs::read_dir(&session_dir) else {
                continue;
            };
            let pids: Vec<u32> = files
                .flatten()
                .filter_map(|f| lock_pid(f.file_name().to_str()?))
                .collect();
            if pids.is_empty() {
                continue;
            }
            let event = |kind| AgentEvent {
                source: EventSource::Scan,
                at: now,
                harness: HarnessId::CopilotCli,
                session: session.to_owned(),
                agent: None,
                dedupe_key: None,
                kind,
            };
            out.extend(
                pids.into_iter()
                    .map(|pid| event(EventKind::Heartbeat { pid })),
            );
            let workspace =
                std::fs::read_to_string(session_dir.join("workspace.yaml")).unwrap_or_default();
            let cwd = yaml_field(&workspace, "cwd");
            if cwd.is_some() {
                out.push(event(EventKind::SessionInfo {
                    cwd,
                    model: None,
                    title: yaml_field(&workspace, "name")
                        .or_else(|| yaml_field(&workspace, "summary")),
                }));
            }
        }
        out
    }

    fn match_process(&self, p: &ProcessInfo) -> Option<ProcessMatch> {
        match_by_name(self.id(), &["copilot"], p)
    }
}

/// `inuse.4242.lock` → 4242.
fn lock_pid(name: &str) -> Option<u32> {
    name.strip_prefix("inuse.")?
        .strip_suffix(".lock")?
        .parse()
        .ok()
}

/// A top-level scalar from a flat YAML file, without pulling in a YAML parser.
fn yaml_field(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let value = line.strip_prefix(key)?.strip_prefix(':')?.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        (!value.is_empty() && !matches!(value, "|" | ">" | "|-" | ">-")).then(|| value.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_lock_files_and_workspace() {
        let home = tempfile::tempdir().unwrap();
        let live = home.path().join("session-state/s-live");
        let done = home.path().join("session-state/s-done");
        std::fs::create_dir_all(&live).unwrap();
        std::fs::create_dir_all(&done).unwrap();
        std::fs::write(live.join("inuse.4242.lock"), "4242").unwrap();
        std::fs::write(
            live.join("workspace.yaml"),
            "id: s-live\ncwd: /work/api\nname: \"Add rate limits\"\nsummary: |\n  long\n",
        )
        .unwrap();
        std::fs::write(done.join("workspace.yaml"), "cwd: /work/old\n").unwrap();

        let events = CopilotCli::with_home(home.path()).scan(Utc::now());
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].session, "s-live");
        assert_eq!(events[0].kind, EventKind::Heartbeat { pid: 4242 });
        assert_eq!(
            events[1].kind,
            EventKind::SessionInfo {
                cwd: Some("/work/api".into()),
                model: None,
                title: Some("Add rate limits".into()),
            }
        );
    }

    #[test]
    fn parses_flat_yaml() {
        assert_eq!(yaml_field("summary: |\n  x", "summary"), None);
        assert_eq!(
            yaml_field("cwd: 'C:\\work'", "cwd").as_deref(),
            Some("C:\\work")
        );
        assert_eq!(yaml_field("cwdx: a", "cwd"), None);
        assert_eq!(lock_pid("inuse.12.lock"), Some(12));
        assert_eq!(lock_pid("inuse.x.lock"), None);
    }
}
