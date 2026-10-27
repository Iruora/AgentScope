//! Adds and removes AgentScope's entries in `~/.cursor/hooks.json`.
//!
//! Cursor lists handlers directly under each event:
//!
//! ```json
//! { "version": 1, "hooks": { "stop": [
//!   { "command": "'/…/agentscope-hook' --harness cursor --event stop", "timeout": 5 } ] } }
//! ```
//!
//! Cursor runs the command through a shell: `sh` on macOS and Linux,
//! PowerShell on Windows, which needs the call operator for a quoted path.

use std::path::Path;

use serde_json::{json, Value};

use super::hooks::EVENTS;
use crate::config::{shell_quote, HookConfig};
use crate::{Diff, HookPlan, HookStatus, Result};

const HOOK_TIMEOUT_SECS: u64 = 5;

fn command(shim: &Path, event: &str) -> String {
    let program = if cfg!(windows) {
        format!("& {}", shell_quote(shim, true))
    } else {
        shell_quote(shim, false)
    };
    format!("{program} --harness cursor --event {event}")
}

fn hook_entry(shim: &Path, event: &str) -> Value {
    json!({
        "command": command(shim, event),
        "timeout": HOOK_TIMEOUT_SECS,
    })
}

const CONFIG: HookConfig<'static> = HookConfig {
    events: EVENTS,
    entry: &hook_entry,
    grouped: false,
    required: &[("version", 1)],
};

pub fn status(path: &Path, shim: &Path) -> HookStatus {
    CONFIG.status(path, shim)
}

pub fn install(path: &Path, plan: &HookPlan) -> Result<Diff> {
    CONFIG.install(path, &plan.shim_path, plan.dry_run)
}

pub fn uninstall(path: &Path, dry_run: bool) -> Result<Diff> {
    CONFIG.uninstall(path, dry_run)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn keeps_the_users_hooks_and_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hooks.json");
        let user = r#"{
  "version": 1,
  "hooks": {
    "afterFileEdit": [{ "command": "./hooks/format.sh" }],
    "stop": [{ "command": "./hooks/notify.sh" }]
  }
}
"#;
        fs::write(&path, user).unwrap();
        let shim = PathBuf::from("/Applications/AgentScope.app/Contents/MacOS/agentscope-hook");
        let plan = HookPlan {
            shim_path: shim.clone(),
            dry_run: false,
        };
        install(&path, &plan).unwrap();
        assert_eq!(status(&path, &shim), HookStatus::Installed);
        let written: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let stop = written["hooks"]["stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0]["command"], "./hooks/notify.sh");
        if !cfg!(windows) {
            assert_eq!(
                stop[1]["command"],
                "'/Applications/AgentScope.app/Contents/MacOS/agentscope-hook' --harness cursor --event stop"
            );
        }

        uninstall(&path, false).unwrap();
        let restored: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(restored, serde_json::from_str::<Value>(user).unwrap());
    }

    #[test]
    fn a_new_file_gets_a_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hooks.json");
        let plan = HookPlan {
            shim_path: PathBuf::from("/x/agentscope-hook"),
            dry_run: false,
        };
        install(&path, &plan).unwrap();
        let written: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["version"], 1);
        uninstall(&path, false).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "{\n  \"version\": 1\n}\n"
        );
    }
}
