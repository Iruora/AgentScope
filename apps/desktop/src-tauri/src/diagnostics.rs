//! Settings → Diagnostics: what each harness has sent since the app started,
//! and a redacted plain-text report to paste into a GitHub issue.

use std::fmt::Write as _;
use std::path::Path;

use agentscope_adapters::{Detection, HookStatus};
use agentscope_core::redact::redact_text;
use agentscope_core::{HarnessId, Snapshot};
use chrono::{DateTime, Utc};
use serde::Serialize;
use specta::Type;

use crate::engine::Signals;

/// The release line each adapter was built and tested against. Keep in step
/// with `tested` in scripts/drift/harnesses.json (a test checks this).
pub fn tested_version(harness: HarnessId) -> &'static str {
    match harness {
        HarnessId::ClaudeCode => "2.1",
        HarnessId::Codex => "0.165",
        HarnessId::CopilotCli => "1.0",
        HarnessId::Cursor => "hooks v1",
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HarnessDiagnostics {
    pub harness: HarnessId,
    pub installed: bool,
    pub version: Option<String>,
    pub tested_version: String,
    pub config_dir: Option<String>,
    pub hook_status: HookStatus,
    pub hooks_received: u32,
    pub unhandled_hooks: u32,
    pub last_hook_at: Option<String>,
    pub last_hook_event: Option<String>,
    pub transcript_events: u32,
    pub last_transcript_at: Option<String>,
    pub live_sessions: u32,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub app_version: String,
    pub os: String,
    pub data_dir: String,
    pub ingest_port: Option<u16>,
    pub shim_path: Option<String>,
    pub spool_files: u32,
    #[specta(type = f64)]
    pub spool_bytes: u64,
    pub harnesses: Vec<HarnessDiagnostics>,
    /// Recent warnings and errors from the app's log, oldest first.
    pub recent_logs: Vec<String>,
}

pub struct Inputs<'a> {
    pub detections: Vec<Detection>,
    pub hook_status: &'a dyn Fn(HarnessId) -> HookStatus,
    pub signals: std::collections::HashMap<HarnessId, Signals>,
    pub snapshot: &'a Snapshot,
    pub data_dir: &'a Path,
    pub ingest_port: Option<u16>,
    pub shim_path: Option<String>,
    pub recent_logs: Vec<String>,
}

fn iso(at: Option<DateTime<Utc>>) -> Option<String> {
    at.map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

fn spool(data_dir: &Path) -> (u32, u64) {
    let Ok(entries) = std::fs::read_dir(agentscope_protocol::spool_dir(data_dir)) else {
        return (0, 0);
    };
    entries
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .fold((0, 0), |(n, bytes), m| (n + 1, bytes + m.len()))
}

pub fn collect(inputs: Inputs<'_>) -> Diagnostics {
    let (spool_files, spool_bytes) = spool(inputs.data_dir);
    let harnesses = inputs
        .detections
        .into_iter()
        .map(|d| {
            let signals = inputs.signals.get(&d.harness).cloned().unwrap_or_default();
            let live_sessions = inputs
                .snapshot
                .sessions
                .iter()
                .filter(|s| s.harness == d.harness && s.ended_at.is_none())
                .count();
            HarnessDiagnostics {
                harness: d.harness,
                installed: d.installed,
                version: d.version,
                tested_version: tested_version(d.harness).to_owned(),
                config_dir: d.config_dir,
                hook_status: (inputs.hook_status)(d.harness),
                hooks_received: signals.hooks,
                unhandled_hooks: signals.unhandled_hooks,
                last_hook_at: iso(signals.last_hook_at),
                last_hook_event: signals.last_hook_event,
                transcript_events: signals.transcript_events,
                last_transcript_at: iso(signals.last_transcript_at),
                live_sessions: u32::try_from(live_sessions).unwrap_or(u32::MAX),
            }
        })
        .collect();
    Diagnostics {
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        data_dir: inputs.data_dir.display().to_string(),
        ingest_port: inputs.ingest_port,
        shim_path: inputs.shim_path,
        spool_files,
        spool_bytes,
        harnesses,
        recent_logs: inputs.recent_logs,
    }
}

/// Plain text for a bug report: home paths become `~` and anything that looks
/// like a secret is masked.
pub fn report(d: &Diagnostics) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "AgentScope {} on {}", d.app_version, d.os);
    let _ = writeln!(out, "Data dir: {}", d.data_dir);
    let _ = writeln!(
        out,
        "Ingest: {}",
        d.ingest_port
            .map_or("not running".to_owned(), |p| format!("127.0.0.1:{p}"))
    );
    let _ = writeln!(
        out,
        "Hook helper: {}",
        d.shim_path.as_deref().unwrap_or("missing")
    );
    let _ = writeln!(
        out,
        "Spool: {} file(s), {} bytes",
        d.spool_files, d.spool_bytes
    );
    for h in &d.harnesses {
        let _ = writeln!(out, "\n[{}]", h.harness);
        let _ = writeln!(
            out,
            "installed: {}, version: {}, tested with: {}",
            h.installed,
            h.version.as_deref().unwrap_or("unknown"),
            h.tested_version
        );
        let _ = writeln!(
            out,
            "config: {}",
            h.config_dir.as_deref().unwrap_or("not found")
        );
        let _ = writeln!(out, "hooks: {:?}", h.hook_status);
        let _ = writeln!(
            out,
            "hook events since start: {} ({} unhandled), last: {} at {}",
            h.hooks_received,
            h.unhandled_hooks,
            h.last_hook_event.as_deref().unwrap_or("none"),
            h.last_hook_at.as_deref().unwrap_or("-")
        );
        let _ = writeln!(
            out,
            "transcript events since start: {}, last at {}",
            h.transcript_events,
            h.last_transcript_at.as_deref().unwrap_or("-")
        );
        let _ = writeln!(out, "live sessions: {}", h.live_sessions);
    }
    if !d.recent_logs.is_empty() {
        let _ = writeln!(out, "\nRecent warnings and errors:");
        for line in &d.recent_logs {
            let _ = writeln!(out, "{line}");
        }
    }
    scrub(&out, dirs_home().as_deref())
}

fn dirs_home() -> Option<String> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(|h| h.to_string_lossy().into_owned())
        .filter(|h| h.len() > 1)
}

fn scrub(text: &str, home: Option<&str>) -> String {
    let text = match home {
        Some(home) => text.replace(home, "~"),
        None => text.to_owned(),
    };
    redact_text(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tested_versions_match_the_drift_check() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../scripts/drift/harnesses.json"
        );
        let config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for harness in HarnessId::ALL {
            if let Some(entry) = config.get(harness.as_str()) {
                assert_eq!(entry["tested"], tested_version(harness), "{harness}");
            }
        }
    }

    #[test]
    fn report_hides_home_and_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = Snapshot {
            sessions: vec![],
            agents: vec![],
            approvals: vec![],
            waiting_count: 0,
            concerns: vec![],
        };
        let mut signals = std::collections::HashMap::new();
        signals.insert(
            HarnessId::ClaudeCode,
            Signals {
                hooks: 4,
                unhandled_hooks: 1,
                last_hook_event: Some("Stop".into()),
                ..Default::default()
            },
        );
        let d = collect(Inputs {
            detections: vec![Detection {
                harness: HarnessId::ClaudeCode,
                installed: true,
                config_dir: Some("/home/dana/.claude".into()),
                version: Some("2.1.316".into()),
            }],
            hook_status: &|_| HookStatus::Installed,
            signals,
            snapshot: &snapshot,
            data_dir: dir.path(),
            ingest_port: Some(4242),
            shim_path: None,
            recent_logs: vec![
                "WARN token=sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789".into(),
            ],
        });
        assert_eq!(d.harnesses[0].hooks_received, 4);
        assert_eq!(d.harnesses[0].tested_version, "2.1");

        let text = scrub(&report(&d), Some("/home/dana"));
        assert!(text.contains("config: ~/.claude"));
        assert!(text.contains("4 (1 unhandled), last: Stop"));
        assert!(!text.contains("abcdefghijklmnopqrstuvwxyz0123456789"));
        assert!(!text.contains("/home/dana"));
    }
}
