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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            notify_approvals: true,
            notify_input: false,
            retention_days: crate::engine::DEFAULT_RETENTION_DAYS,
            onboarded: false,
            check_updates: true,
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
        };
        save(dir.path(), &off).unwrap();
        assert_eq!(load(dir.path()), off);
    }
}
