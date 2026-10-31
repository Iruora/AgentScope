//! The optional global shortcut that jumps to the agent that has waited
//! longest for the user, from any app.

use std::sync::Arc;

use agentscope_core::Snapshot;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::engine::Engine;

/// Shortcuts Settings offers. Empty means off.
pub const CHOICES: &[&str] = &[
    "CommandOrControl+Shift+Y",
    "CommandOrControl+Alt+A",
    "CommandOrControl+Shift+Period",
];

/// The live session to jump to: the oldest pending approval first, then the
/// session that has waited longest for input.
pub fn next_waiting(snapshot: &Snapshot) -> Option<String> {
    let live = |id: &str| {
        snapshot
            .sessions
            .iter()
            .any(|s| s.id == id && s.ended_at.is_none())
    };
    if let Some(approval) = snapshot.approvals.iter().find(|a| live(&a.session_id)) {
        return Some(approval.session_id.clone());
    }
    snapshot
        .sessions
        .iter()
        .filter(|s| s.ended_at.is_none() && s.state.needs_user())
        .min_by_key(|s| s.last_event_at)
        .map(|s| s.id.clone())
}

/// Replaces the registered shortcut. An empty string turns it off.
pub fn apply(app: &AppHandle, accelerator: &str) -> Result<(), String> {
    if !accelerator.is_empty() && !CHOICES.contains(&accelerator) {
        return Err(format!("{accelerator} is not one of the offered shortcuts"));
    }
    let shortcuts = app.global_shortcut();
    shortcuts.unregister_all().map_err(|e| e.to_string())?;
    if accelerator.is_empty() {
        return Ok(());
    }
    let shortcut: Shortcut = accelerator
        .parse()
        .map_err(|e| format!("{accelerator} is not a valid shortcut: {e}"))?;
    shortcuts
        .on_shortcut(shortcut, |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                jump(app);
            }
        })
        .map_err(|e| format!("Could not use {accelerator}, another app may own it: {e}"))
}

fn jump(app: &AppHandle) {
    let engine = app.state::<Arc<Engine>>().inner().clone();
    let Some(session_id) = next_waiting(&engine.snapshot()) else {
        crate::show_main_window(app);
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let focused = engine
            .locate(&session_id)
            .is_some_and(|location| crate::focus::go_to(&location).focused);
        // When the terminal cannot be focused, the session page shows how to get there.
        if !focused {
            crate::open_session(&app, &session_id);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentscope_core::{AgentState, HarnessId, Session};
    use chrono::{Duration, Utc};

    fn session(id: &str, state: AgentState, minutes_ago: i64, ended: bool) -> Session {
        let at = Utc::now() - Duration::minutes(minutes_ago);
        let mut s: Session = serde_json::from_value(serde_json::json!({
            "id": id, "harness": "claude-code", "nativeId": id, "title": null, "cwd": "/w",
            "repo": null, "model": null, "startedAt": at, "lastEventAt": at,
            "endedAt": null, "state": "idle", "pid": null, "terminal": null,
            "transcriptPath": null, "tokens": null, "rootAgentId": format!("{id}/main"),
        }))
        .unwrap();
        s.state = state;
        if ended {
            s.ended_at = Some(at);
        }
        assert_eq!(s.harness, HarnessId::ClaudeCode);
        s
    }

    fn snapshot(sessions: Vec<Session>) -> Snapshot {
        Snapshot {
            sessions,
            agents: vec![],
            approvals: vec![],
            waiting_count: 0,
            concerns: vec![],
        }
    }

    #[test]
    fn picks_the_longest_waiting_live_session() {
        let snap = snapshot(vec![
            session("busy", AgentState::RunningTool, 30, false),
            session("recent", AgentState::WaitingInput, 1, false),
            session("old", AgentState::WaitingInput, 9, false),
            session("gone", AgentState::WaitingInput, 60, true),
        ]);
        assert_eq!(next_waiting(&snap).as_deref(), Some("old"));
        assert_eq!(next_waiting(&snapshot(vec![])), None);
    }

    #[test]
    fn choices_parse_and_match_the_settings_page() {
        let page = include_str!("../../src/views/Settings.tsx");
        for choice in CHOICES {
            choice.parse::<Shortcut>().unwrap();
            assert!(
                page.contains(&format!("\"{choice}\"")),
                "{choice} missing from Settings.tsx"
            );
        }
    }
}
