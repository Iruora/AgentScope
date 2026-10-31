//! Claude Code: hooks in `~/.claude/settings.json`, transcripts under
//! `~/.claude/projects`, live sessions in `~/.claude/sessions/<pid>.json`.
//! Notes on payload shapes are in `docs/adapters/claude-code.md`.

mod hooks;
mod install;
mod transcript;

use std::path::{Path, PathBuf};

use agentscope_core::{AgentEvent, EventKind, EventSource, HarnessId, RawHookEvent, Timestamp};
use chrono::Utc;

use crate::*;

/// Env var Claude Code reads to move its config dir away from `~/.claude`.
const CONFIG_DIR_ENV: &str = "CLAUDE_CONFIG_DIR";

pub struct ClaudeCode {
    config_dir: Option<PathBuf>,
}

impl ClaudeCode {
    /// Uses `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
    pub fn new() -> Self {
        let config_dir = std::env::var_os(CONFIG_DIR_ENV)
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .or_else(|| home_dir(".claude"));
        Self { config_dir }
    }

    pub fn with_config_dir(dir: impl Into<PathBuf>) -> Self {
        Self {
            config_dir: Some(dir.into()),
        }
    }

    fn settings_path(&self) -> Result<PathBuf> {
        self.config_dir
            .as_ref()
            .map(|d| d.join("settings.json"))
            .ok_or_else(|| AdapterError::NotFound("the Claude Code config directory".into()))
    }
}

impl Default for ClaudeCode {
    fn default() -> Self {
        Self::new()
    }
}

impl Adapter for ClaudeCode {
    fn id(&self) -> HarnessId {
        HarnessId::ClaudeCode
    }

    fn detect(&self) -> Detection {
        detect_config_dir(self.id(), self.config_dir.clone())
    }

    fn hooks_status(&self, shim_path: &Path) -> HookStatus {
        match self.settings_path() {
            Ok(path) => install::status(&path, shim_path),
            Err(_) => HookStatus::NotInstalled,
        }
    }

    fn install_hooks(&self, plan: &HookPlan) -> Result<Diff> {
        install::install(&self.settings_path()?, plan)
    }

    fn uninstall_hooks(&self, dry_run: bool) -> Result<Diff> {
        install::uninstall(&self.settings_path()?, dry_run)
    }

    fn can_answer_approvals(&self) -> bool {
        true
    }

    fn answers_approvals(&self) -> bool {
        self.settings_path()
            .is_ok_and(|path| install::answers(&path))
    }

    fn normalize_hook(&self, raw: &RawHookEvent) -> Vec<AgentEvent> {
        hooks::normalize(raw)
    }

    fn transcript_roots(&self) -> Vec<WatchRoot> {
        self.config_dir
            .iter()
            .map(|d| WatchRoot {
                path: d.join("projects"),
                recursive: true,
                suffix: ".jsonl",
            })
            .collect()
    }

    fn parse_transcript_chunk(&self, file: &TranscriptFile, bytes: &[u8]) -> Vec<AgentEvent> {
        transcript::parse(file, bytes)
    }

    /// Claude Code keeps `sessions/<pid>.json` for each running session. The
    /// engine drops heartbeats whose process is gone.
    fn scan(&self, _since: Timestamp) -> Vec<AgentEvent> {
        let Some(dir) = self.config_dir.as_ref().map(|d| d.join("sessions")) else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let now = Utc::now();
        let mut events = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Some(live) = std::fs::read(&path)
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            else {
                continue;
            };
            let (Some(pid), Some(session)) = (
                live.get("pid").and_then(|v| v.as_u64()),
                live.get("sessionId").and_then(|v| v.as_str()),
            ) else {
                continue;
            };
            let event = |kind| AgentEvent {
                source: EventSource::Scan,
                at: now,
                harness: HarnessId::ClaudeCode,
                session: session.to_owned(),
                agent: None,
                dedupe_key: None,
                kind,
            };
            events.push(event(EventKind::Heartbeat { pid: pid as u32 }));
            if let Some(cwd) = live.get("cwd").and_then(|v| v.as_str()) {
                events.push(event(EventKind::SessionInfo {
                    cwd: Some(cwd.to_owned()),
                    model: None,
                    title: None,
                }));
            }
        }
        events
    }

    /// The native installer runs a `claude` binary; npm installs run
    /// `node …/@anthropic-ai/claude-code/cli.js`.
    fn match_process(&self, p: &ProcessInfo) -> Option<ProcessMatch> {
        match_by_name(self.id(), &["claude"], p).or_else(|| {
            let exe = Path::new(&p.name).file_stem()?.to_str()?;
            let runs_cli = matches!(exe, "node" | "bun")
                && p.cmd.iter().skip(1).take(2).any(|arg| {
                    arg.contains("claude-code")
                        || Path::new(arg).file_stem().is_some_and(|s| s == "claude")
                });
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
    fn matches_native_and_npm_installs() {
        let cc = ClaudeCode::with_config_dir("/nope");
        assert!(cc.match_process(&process("claude", &["claude"])).is_some());
        assert!(cc
            .match_process(&process(
                "node",
                &[
                    "node",
                    "/usr/lib/node_modules/@anthropic-ai/claude-code/cli.js"
                ]
            ))
            .is_some());
        assert!(cc
            .match_process(&process("node", &["node", "/usr/local/bin/claude", "-p"]))
            .is_some());
        assert!(cc
            .match_process(&process("node", &["node", "server.js"]))
            .is_none());
    }

    #[test]
    fn scans_live_session_files() {
        let dir = tempfile::tempdir().unwrap();
        let sessions = dir.path().join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(
            sessions.join("4242.json"),
            r#"{"pid":4242,"sessionId":"s1","cwd":"/repo","status":"busy"}"#,
        )
        .unwrap();
        std::fs::write(sessions.join("4242.abc.key"), "secret").unwrap();
        std::fs::write(sessions.join("bad.json"), "{").unwrap();

        let events = ClaudeCode::with_config_dir(dir.path()).scan(Utc::now());
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, EventKind::Heartbeat { pid: 4242 });
        assert_eq!(events[0].session, "s1");
        assert!(
            matches!(&events[1].kind, EventKind::SessionInfo { cwd: Some(c), .. } if c == "/repo")
        );
    }
}
