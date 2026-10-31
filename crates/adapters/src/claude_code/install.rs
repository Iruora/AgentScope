//! Adds and removes AgentScope's entries in `~/.claude/settings.json`.
//!
//! Each registered event gets its own matcher group holding one hook:
//!
//! ```json
//! { "hooks": [{ "type": "command", "command": "/…/agentscope-hook",
//!               "args": ["--harness", "claude-code", "--event", "Stop"],
//!               "async": true, "timeout": 10 }] }
//! ```
//!
//! The exec form (`args`) runs the shim without a shell, so paths with spaces
//! work on every OS and the shim's parent is Claude Code itself. `async` makes
//! Claude Code run it in the background, so it can never delay the agent.
//! Our entries are recognized by the shim's file name, so Disconnect removes
//! exactly them even after the app moved, and nothing else is touched.

use std::path::Path;

use serde_json::{json, Value};

use super::hooks::EVENTS;
use crate::config::HookConfig;
use crate::{Diff, HookPlan, HookStatus, Result};

const HOOK_TIMEOUT_SECS: u64 = 10;

/// Longer than the app's longest decide window, so Claude Code never kills
/// the shim before AgentScope lets go of the prompt.
const DECIDE_TIMEOUT_SECS: u64 = agentscope_core::DECIDE_MAX_WAIT_SECS + 10;

fn hook_entry(shim: &Path, event: &str) -> Value {
    json!({
        "type": "command",
        "command": shim.display().to_string(),
        "args": ["--harness", "claude-code", "--event", event],
        "async": true,
        "timeout": HOOK_TIMEOUT_SECS,
    })
}

/// With approvals answered from AgentScope, `PermissionRequest` waits for the
/// user's answer instead of running in the background. If AgentScope gives no
/// answer, the shim prints nothing and Claude Code asks as usual.
fn decide_entry(shim: &Path, event: &str) -> Option<Value> {
    (event == "PermissionRequest").then(|| {
        json!({
            "type": "command",
            "command": shim.display().to_string(),
            "args": ["--harness", "claude-code", "--event", event, "--decide"],
            "timeout": DECIDE_TIMEOUT_SECS,
        })
    })
}

const CONFIG: HookConfig<'static> = HookConfig {
    events: EVENTS,
    entry: &hook_entry,
    grouped: true,
    required: &[],
    decide_entry: Some(&decide_entry),
};

pub fn answers(path: &Path) -> bool {
    CONFIG.answers(path)
}

pub fn status(path: &Path, shim: &Path) -> HookStatus {
    CONFIG.status(path, shim)
}

pub fn install(path: &Path, plan: &HookPlan) -> Result<Diff> {
    CONFIG.install(path, &plan.shim_path, plan.dry_run, plan.answer_approvals)
}

pub fn uninstall(path: &Path, dry_run: bool) -> Result<Diff> {
    CONFIG.uninstall(path, dry_run)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AdapterError;
    use std::fs;
    use std::path::PathBuf;

    const USER_SETTINGS: &str = r#"{
  "model": "opus",
  "hooks": {
    "PreToolUse": [
      { "matcher": "Bash", "hooks": [{ "type": "command", "command": "~/bin/audit.sh" }] }
    ]
  },
  "permissions": { "allow": ["Bash(ls:*)"] }
}
"#;

    fn plan(shim: &str, dry_run: bool) -> HookPlan {
        HookPlan {
            shim_path: PathBuf::from(shim),
            dry_run,
            answer_approvals: false,
        }
    }

    #[test]
    fn install_is_idempotent_and_uninstall_restores_the_user_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, USER_SETTINGS).unwrap();
        let shim = "/Applications/Agent Scope.app/Contents/MacOS/agentscope-hook";

        assert_eq!(status(&path, Path::new(shim)), HookStatus::NotInstalled);
        let preview = install(&path, &plan(shim, true)).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            USER_SETTINGS,
            "dry run writes nothing"
        );
        assert_eq!(preview.before.as_deref(), Some(USER_SETTINGS));

        let diff = install(&path, &plan(shim, false)).unwrap();
        assert_eq!(diff.after, preview.after);
        assert!(diff.backup_path.is_some());
        assert_eq!(status(&path, Path::new(shim)), HookStatus::Installed);

        let written: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let pre = written["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 2, "the user's own hook is kept");
        assert_eq!(pre[0]["matcher"], "Bash");
        assert_eq!(pre[1]["hooks"][0]["command"], shim);
        assert_eq!(pre[1]["hooks"][0]["async"], true);
        assert_eq!(written["model"], "opus");
        let keys: Vec<_> = written.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["model", "hooks", "permissions"], "key order is kept");

        assert!(install(&path, &plan(shim, false)).unwrap().is_noop());

        let removed = uninstall(&path, false).unwrap();
        assert!(!removed.is_noop());
        let restored: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            restored,
            serde_json::from_str::<Value>(USER_SETTINGS).unwrap()
        );
        assert_eq!(status(&path, Path::new(shim)), HookStatus::NotInstalled);
        assert_eq!(
            fs::read_to_string(diff.backup_path.unwrap()).unwrap(),
            USER_SETTINGS
        );
    }

    #[test]
    fn a_moved_shim_is_outdated_and_reinstall_replaces_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        install(&path, &plan("/old/agentscope-hook", false)).unwrap();
        assert_eq!(
            status(&path, Path::new("/new/agentscope-hook")),
            HookStatus::Outdated
        );

        install(&path, &plan(r"C:\Apps\agentscope-hook.exe", false)).unwrap();
        let written: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["hooks"]["Stop"].as_array().unwrap().len(), 1);
        assert_eq!(
            status(&path, Path::new(r"C:\Apps\agentscope-hook.exe")),
            HookStatus::Installed
        );

        uninstall(&path, false).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}\n");
    }

    #[test]
    fn answering_approvals_swaps_only_the_permission_hook() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let shim = "/x/agentscope-hook";
        install(&path, &plan(shim, false)).unwrap();
        assert!(!answers(&path));

        let answering = HookPlan {
            answer_approvals: true,
            ..plan(shim, false)
        };
        install(&path, &answering).unwrap();
        assert!(answers(&path));
        assert_eq!(status(&path, Path::new(shim)), HookStatus::Installed);
        let written: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let perm = &written["hooks"]["PermissionRequest"][0]["hooks"][0];
        assert_eq!(perm["args"][4], "--decide");
        assert!(perm.get("async").is_none(), "it must block to answer");
        assert!(perm["timeout"].as_u64().unwrap() > agentscope_core::DECIDE_MAX_WAIT_SECS);
        let stop = &written["hooks"]["Stop"][0]["hooks"][0];
        assert_eq!(
            stop["async"], true,
            "everything else stays in the background"
        );
        assert_eq!(
            written["hooks"]["PermissionRequest"]
                .as_array()
                .unwrap()
                .len(),
            1
        );

        install(&path, &plan(shim, false)).unwrap();
        assert!(!answers(&path));
        uninstall(&path, false).unwrap();
        assert!(!answers(&path));
    }

    #[test]
    fn creates_a_missing_file_and_refuses_invalid_ones() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/settings.json");
        let diff = install(&path, &plan("/x/agentscope-hook", false)).unwrap();
        assert_eq!(diff.before, None);
        assert_eq!(diff.backup_path, None);
        assert!(path.exists());

        fs::write(&path, "{ not json").unwrap();
        assert!(matches!(
            install(&path, &plan("/x/agentscope-hook", false)),
            Err(AdapterError::InvalidConfig { .. })
        ));
        fs::write(&path, r#"{ "hooks": [] }"#).unwrap();
        assert!(install(&path, &plan("/x/agentscope-hook", false)).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), r#"{ "hooks": [] }"#);
    }
}
