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

use std::fs;
use std::path::Path;

use serde_json::{json, Map, Value};

use super::hooks::EVENTS;
use crate::{AdapterError, Diff, HookPlan, HookStatus, Result};

const SHIM_STEM: &str = "agentscope-hook";
const HOOK_TIMEOUT_SECS: u64 = 10;
const BACKUP_SUFFIX: &str = "agentscope-backup";

fn hook_entry(shim: &Path, event: &str) -> Value {
    json!({
        "type": "command",
        "command": shim.display().to_string(),
        "args": ["--harness", "claude-code", "--event", event],
        "async": true,
        "timeout": HOOK_TIMEOUT_SECS,
    })
}

fn is_ours(hook: &Value) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .and_then(|c| {
            // Tolerate both separators: the file may come from another OS.
            let name = c.trim().trim_matches('"').rsplit(['/', '\\']).next()?;
            Some(name.strip_suffix(".exe").unwrap_or(name) == SHIM_STEM)
        })
        .unwrap_or(false)
}

fn read(path: &Path) -> Result<Option<(String, Map<String, Value>)>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    if text.trim().is_empty() {
        return Ok(Some((text, Map::new())));
    }
    let invalid = |reason: String| AdapterError::InvalidConfig {
        path: path.display().to_string(),
        reason,
    };
    match serde_json::from_str::<Value>(&text).map_err(|e| invalid(e.to_string()))? {
        Value::Object(map) => Ok(Some((text, map))),
        _ => Err(invalid("the top level is not an object".into())),
    }
}

/// The `hooks` object, or an error if it exists with an unexpected shape.
fn hooks_mut<'a>(
    settings: &'a mut Map<String, Value>,
    path: &Path,
) -> Result<&'a mut Map<String, Value>> {
    let hooks = settings
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    hooks
        .as_object_mut()
        .ok_or_else(|| AdapterError::InvalidConfig {
            path: path.display().to_string(),
            reason: "`hooks` is not an object".into(),
        })
}

/// Removes every hook of ours, dropping groups and events left empty.
fn strip_ours(hooks: &mut Map<String, Value>) {
    hooks.retain(|_, groups| {
        let Some(list) = groups.as_array_mut() else {
            return true;
        };
        let before = list.len();
        list.retain_mut(|group| {
            let Some(inner) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            let had = inner.len();
            inner.retain(|h| !is_ours(h));
            !(had > 0 && inner.is_empty())
        });
        !(before > 0 && list.is_empty())
    });
}

fn render(settings: &Map<String, Value>) -> Result<String> {
    Ok(serde_json::to_string_pretty(settings)? + "\n")
}

pub fn status(path: &Path, shim: &Path) -> HookStatus {
    let Ok(Some((_, settings))) = read(path) else {
        return HookStatus::NotInstalled;
    };
    let Some(hooks) = settings.get("hooks").and_then(Value::as_object) else {
        return HookStatus::NotInstalled;
    };
    let ours = |event: &str| -> Vec<&Value> {
        hooks
            .get(event)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|g| g.get("hooks").and_then(Value::as_array))
            .flatten()
            .filter(|h| is_ours(h))
            .collect()
    };
    let any_ours = hooks.keys().any(|event| !ours(event).is_empty());
    let current = EVENTS
        .iter()
        .all(|event| ours(event) == [&hook_entry(shim, event)]);
    match (any_ours, current) {
        (false, _) => HookStatus::NotInstalled,
        (true, true) => HookStatus::Installed,
        (true, false) => HookStatus::Outdated,
    }
}

pub fn install(path: &Path, plan: &HookPlan) -> Result<Diff> {
    let (before, mut settings) = match read(path)? {
        Some((text, map)) => (Some(text), map),
        None => (None, Map::new()),
    };
    let hooks = hooks_mut(&mut settings, path)?;
    // Reinstalling replaces old entries, e.g. after the app moved.
    strip_ours(hooks);
    for event in EVENTS {
        let groups = hooks
            .entry(*event)
            .or_insert_with(|| Value::Array(Vec::new()));
        let Some(groups) = groups.as_array_mut() else {
            return Err(AdapterError::InvalidConfig {
                path: path.display().to_string(),
                reason: format!("`hooks.{event}` is not a list"),
            });
        };
        groups.push(json!({ "hooks": [hook_entry(&plan.shim_path, event)] }));
    }
    finish(path, before, &settings, plan.dry_run)
}

pub fn uninstall(path: &Path, dry_run: bool) -> Result<Diff> {
    let Some((before, mut settings)) = read(path)? else {
        return Ok(Diff {
            path: path.display().to_string(),
            before: None,
            after: None,
            backup_path: None,
        });
    };
    if let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) {
        strip_ours(hooks);
        if hooks.is_empty() {
            settings.remove("hooks");
        }
    }
    finish(path, Some(before), &settings, dry_run)
}

fn finish(
    path: &Path,
    before: Option<String>,
    settings: &Map<String, Value>,
    dry_run: bool,
) -> Result<Diff> {
    let after = render(settings)?;
    // Unchanged content keeps the user's own formatting.
    let unchanged = before
        .as_deref()
        .and_then(|b| serde_json::from_str::<Value>(b).ok())
        == Some(Value::Object(settings.clone()));
    let after = if unchanged {
        before.clone()
    } else {
        Some(after)
    };
    let mut diff = Diff {
        path: path.display().to_string(),
        before,
        after,
        backup_path: None,
    };
    if dry_run || diff.is_noop() {
        return Ok(diff);
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    if diff.before.is_some() {
        let backup = path.with_extension(format!("json.{BACKUP_SUFFIX}"));
        // Keep the very first original; later edits only ever touch our entries.
        if !backup.exists() {
            fs::copy(path, &backup)?;
        }
        diff.backup_path = Some(backup.display().to_string());
    }
    let tmp = path.with_extension("json.agentscope-tmp");
    fs::write(&tmp, diff.after.as_deref().unwrap_or_default())?;
    fs::rename(&tmp, path)?;
    Ok(diff)
}

#[cfg(test)]
mod tests {
    use super::*;
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
