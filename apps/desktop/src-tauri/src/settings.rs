//! User preferences, stored as `settings.json` in the data dir.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Notify when an agent waits for a permission prompt.
    pub notify_approvals: bool,
    /// Notify when an agent finishes a turn or asks a question.
    pub notify_input: bool,
    /// How long ended sessions stay in History.
    pub retention_days: u32,
    /// The first-run welcome was finished or skipped.
    pub onboarded: bool,
    /// Look for new versions on launch and every few hours.
    pub check_updates: bool,
    /// Global shortcut that jumps to the agent waiting longest, e.g.
    /// `CommandOrControl+Shift+Y`. Empty when off.
    pub global_shortcut: String,
    /// How long a permission prompt waits for an answer in AgentScope before
    /// Claude Code asks in the terminal instead.
    pub decide_window_secs: u32,
    /// Ask for a second click before approving a command that looks risky.
    pub confirm_risky: bool,
}

/// The decide windows Settings offers, in seconds.
pub const DECIDE_WINDOWS: &[u32] = &[15, 30, 60, 120];
pub const DEFAULT_DECIDE_WINDOW: u32 = 60;

impl Settings {
    /// The decide window, falling back to the default for values Settings does not offer.
    pub fn decide_window(&self) -> std::time::Duration {
        let secs = if DECIDE_WINDOWS.contains(&self.decide_window_secs) {
            self.decide_window_secs
        } else {
            DEFAULT_DECIDE_WINDOW
        };
        std::time::Duration::from_secs(secs.into())
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            notify_approvals: true,
            notify_input: false,
            retention_days: crate::engine::DEFAULT_RETENTION_DAYS,
            onboarded: false,
            check_updates: true,
            global_shortcut: String::new(),
            decide_window_secs: DEFAULT_DECIDE_WINDOW,
            confirm_risky: true,
        }
    }
}

pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join("settings.json")
}

/// Missing or unreadable settings fall back to the defaults.
pub fn load(data_dir: &Path) -> Settings {
    std::fs::read(path(data_dir))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save(data_dir: &Path, settings: &Settings) -> std::io::Result<()> {
    let tmp = path(data_dir).with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(settings)?)?;
    std::fs::rename(tmp, path(data_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_defaults_missing_fields() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()), Settings::default());
        std::fs::write(path(dir.path()), r#"{ "notifyInput": true }"#).unwrap();
        assert_eq!(
            load(dir.path()),
            Settings {
                notify_input: true,
                ..Settings::default()
            }
        );
        let off = Settings {
            notify_approvals: false,
            notify_input: false,
            retention_days: 30,
            onboarded: true,
            check_updates: false,
            global_shortcut: "CommandOrControl+Shift+Y".into(),
            decide_window_secs: 30,
            confirm_risky: false,
        };
        save(dir.path(), &off).unwrap();
        assert_eq!(load(dir.path()), off);
        assert_eq!(off.decide_window().as_secs(), 30);
        let odd = Settings {
            decide_window_secs: 7,
            ..Settings::default()
        };
        assert_eq!(odd.decide_window().as_secs(), 60);
    }
}
