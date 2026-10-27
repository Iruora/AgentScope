//! Writes and removes `~/.copilot/hooks/agentscope.json`.
//!
//! Copilot CLI loads every file in its hooks directory, so AgentScope owns a
//! file of its own: Connect writes it whole and Disconnect deletes it,
//! without touching the user's settings. Handlers use the `exec` form, which
//! runs the shim without a shell on every OS.

use std::fs;
use std::path::Path;

use serde_json::{json, Map, Value};

use super::hooks::EVENTS;
use crate::config::write_atomic;
use crate::{AdapterError, Diff, HookPlan, HookStatus, Result};

pub const FILE_NAME: &str = "agentscope.json";

const HOOK_TIMEOUT_SECS: u64 = 5;

fn render(shim: &Path) -> String {
    let hooks: Map<String, Value> = EVENTS
        .iter()
        .map(|event| {
            let entry = json!({
                "type": "command",
                "exec": shim.display().to_string(),
                "args": ["--harness", "copilot-cli", "--event", event],
                "timeoutSec": HOOK_TIMEOUT_SECS,
            });
            (event.to_string(), json!([entry]))
        })
        .collect();
    let file = json!({ "version": 1, "hooks": hooks });
    serde_json::to_string_pretty(&file).unwrap_or_default() + "\n"
}

fn read(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

pub fn status(path: &Path, shim: &Path) -> HookStatus {
    match read(path) {
        Ok(Some(text)) if text == render(shim) => HookStatus::Installed,
        Ok(Some(_)) => HookStatus::Outdated,
        _ => HookStatus::NotInstalled,
    }
}

pub fn install(path: &Path, plan: &HookPlan) -> Result<Diff> {
    let diff = Diff {
        path: path.display().to_string(),
        before: read(path)?,
        after: Some(render(&plan.shim_path)),
        backup_path: None,
    };
    if !plan.dry_run && !diff.is_noop() {
        write_atomic(path, diff.after.as_deref().unwrap_or_default())?;
    }
    Ok(diff)
}

pub fn uninstall(path: &Path, dry_run: bool) -> Result<Diff> {
    let diff = Diff {
        path: path.display().to_string(),
        before: read(path)?,
        after: None,
        backup_path: None,
    };
    if !dry_run && diff.before.is_some() {
        fs::remove_file(path).map_err(AdapterError::from)?;
    }
    Ok(diff)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn owns_a_dedicated_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hooks").join(FILE_NAME);
        let shim = PathBuf::from("/opt/AgentScope/agentscope-hook");
        assert_eq!(status(&path, &shim), HookStatus::NotInstalled);

        let plan = HookPlan {
            shim_path: shim.clone(),
            dry_run: false,
        };
        let diff = install(&path, &plan).unwrap();
        assert_eq!(diff.before, None);
        assert_eq!(status(&path, &shim), HookStatus::Installed);
        let written: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["version"], 1);
        let entry = &written["hooks"]["permissionRequest"][0];
        assert_eq!(entry["exec"], "/opt/AgentScope/agentscope-hook");
        assert_eq!(entry["args"][3], "permissionRequest");
        assert!(written["hooks"].get("preToolUse").is_none());
        assert!(install(&path, &plan).unwrap().is_noop());

        assert_eq!(
            status(&path, Path::new("/elsewhere/agentscope-hook")),
            HookStatus::Outdated
        );
        let removed = uninstall(&path, false).unwrap();
        assert_eq!(removed.after, None);
        assert!(!path.exists());
        assert!(uninstall(&path, false).unwrap().is_noop());
    }
}
