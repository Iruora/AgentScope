//! Adds and removes AgentScope's entries in a harness's JSON hook config.
//!
//! Claude Code, Codex and Cursor all keep hooks as `{ "hooks": { <event>:
//! [...] } }`. Claude Code and Codex wrap handlers in matcher groups
//! (`{ "hooks": [handler] }`); Cursor lists handlers directly. Our handlers
//! are recognized by the shim's file name in their command, so Disconnect
//! removes exactly them even after the app moved, and nothing else is touched.

use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

use crate::{AdapterError, Diff, HookStatus, Result};

pub(crate) const SHIM_STEM: &str = "agentscope-hook";
const BACKUP_SUFFIX: &str = "agentscope-backup";

/// Builds the decide handler for an event, or `None` where the harness takes no answer.
pub(crate) type DecideEntry<'a> = &'a dyn Fn(&Path, &str) -> Option<Value>;

/// One harness's hook file layout.
pub(crate) struct HookConfig<'a> {
    pub events: &'a [&'a str],
    /// The handler AgentScope registers for `event`.
    pub entry: &'a dyn Fn(&Path, &str) -> Value,
    /// Handlers sit in `{ "hooks": [...] }` matcher groups.
    pub grouped: bool,
    /// Top-level keys the file needs, added when missing (Cursor's `version`).
    pub required: &'a [(&'a str, i64)],
    /// The handler that waits for the user's answer from AgentScope, for the
    /// events where the harness accepts one. Used instead of `entry` when
    /// answering approvals is turned on.
    pub decide_entry: Option<DecideEntry<'a>>,
}

/// Whether a handler of ours runs the shim in `--decide` mode.
fn is_decide(hook: &Value) -> bool {
    let in_args = hook
        .get("args")
        .and_then(Value::as_array)
        .is_some_and(|a| a.iter().any(|v| v == "--decide"));
    let in_command = hook
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| c.split_whitespace().any(|w| w == "--decide"));
    in_args || in_command
}

/// Whether `command` runs the shim: a path or shell line whose program is
/// `agentscope-hook`, with either separator since the file may come from
/// another OS.
pub(crate) fn runs_shim(command: &str) -> bool {
    command.match_indices(SHIM_STEM).any(|(at, _)| {
        let before = command[..at].chars().next_back();
        let after = &command[at + SHIM_STEM.len()..];
        let after = after.strip_prefix(".exe").unwrap_or(after);
        matches!(before, None | Some('/' | '\\' | '\'' | '"'))
            && after
                .chars()
                .next()
                .is_none_or(|c| c.is_whitespace() || c == '\'' || c == '"')
    })
}

fn is_ours(hook: &Value) -> bool {
    ["command", "exec", "bash", "powershell"]
        .iter()
        .filter_map(|k| hook.get(*k).and_then(Value::as_str))
        .any(runs_shim)
}

pub(crate) fn read(path: &Path) -> Result<Option<(String, Map<String, Value>)>> {
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

impl HookConfig<'_> {
    /// The handler for `event`: the deciding one when `answer` is on and the
    /// event supports it.
    fn entry_for(&self, shim: &Path, event: &str, answer: bool) -> Value {
        answer
            .then(|| self.decide_entry.and_then(|f| f(shim, event)))
            .flatten()
            .unwrap_or_else(|| (self.entry)(shim, event))
    }

    /// Whether the installed hooks answer approvals from AgentScope.
    pub fn answers(&self, path: &Path) -> bool {
        let Ok(Some((_, settings))) = read(path) else {
            return false;
        };
        let Some(hooks) = settings.get("hooks").and_then(Value::as_object) else {
            return false;
        };
        hooks
            .keys()
            .any(|event| self.ours(hooks, event).iter().any(|(_, h)| is_decide(h)))
    }

    /// Our handlers registered for `event`, with their group index.
    pub fn ours<'v>(&self, hooks: &'v Map<String, Value>, event: &str) -> Vec<(usize, &'v Value)> {
        let list = hooks.get(event).and_then(Value::as_array);
        let mut out = Vec::new();
        for (index, item) in list.into_iter().flatten().enumerate() {
            if self.grouped {
                let inner = item.get("hooks").and_then(Value::as_array);
                out.extend(
                    inner
                        .into_iter()
                        .flatten()
                        .filter(|h| is_ours(h))
                        .map(|h| (index, h)),
                );
            } else if is_ours(item) {
                out.push((index, item));
            }
        }
        out
    }

    pub fn status(&self, path: &Path, shim: &Path) -> HookStatus {
        let Ok(Some((_, settings))) = read(path) else {
            return HookStatus::NotInstalled;
        };
        let Some(hooks) = settings.get("hooks").and_then(Value::as_object) else {
            return HookStatus::NotInstalled;
        };
        let any_ours = hooks
            .keys()
            .any(|event| !self.ours(hooks, event).is_empty());
        let current = self.events.iter().all(|event| {
            let ours: Vec<&Value> = self
                .ours(hooks, event)
                .into_iter()
                .map(|(_, h)| h)
                .collect();
            [false, true]
                .iter()
                .any(|&answer| ours == [&self.entry_for(shim, event, answer)])
        });
        match (any_ours, current) {
            (false, _) => HookStatus::NotInstalled,
            (true, true) => HookStatus::Installed,
            (true, false) => HookStatus::Outdated,
        }
    }

    /// Removes every handler of ours, dropping groups and events left empty.
    fn strip_ours(&self, hooks: &mut Map<String, Value>) {
        hooks.retain(|_, list| {
            let Some(list) = list.as_array_mut() else {
                return true;
            };
            let before = list.len();
            if self.grouped {
                list.retain_mut(|group| {
                    let Some(inner) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                        return true;
                    };
                    let had = inner.len();
                    inner.retain(|h| !is_ours(h));
                    !(had > 0 && inner.is_empty())
                });
            } else {
                list.retain(|h| !is_ours(h));
            }
            !(before > 0 && list.is_empty())
        });
    }

    pub fn install(&self, path: &Path, shim: &Path, dry_run: bool, answer: bool) -> Result<Diff> {
        let (before, mut settings) = match read(path)? {
            Some((text, map)) => (Some(text), map),
            None => (None, Map::new()),
        };
        for (key, value) in self.required {
            settings.entry(*key).or_insert_with(|| Value::from(*value));
        }
        let invalid = |reason: String| AdapterError::InvalidConfig {
            path: path.display().to_string(),
            reason,
        };
        let hooks = settings
            .entry("hooks")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| invalid("`hooks` is not an object".into()))?;
        // Reinstalling replaces old entries, e.g. after the app moved.
        self.strip_ours(hooks);
        for event in self.events {
            let list = hooks
                .entry(*event)
                .or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut()
                .ok_or_else(|| invalid(format!("`hooks.{event}` is not a list")))?;
            let entry = self.entry_for(shim, event, answer);
            list.push(if self.grouped {
                serde_json::json!({ "hooks": [entry] })
            } else {
                entry
            });
        }
        finish(path, before, &settings, dry_run)
    }

    pub fn uninstall(&self, path: &Path, dry_run: bool) -> Result<Diff> {
        let Some((before, mut settings)) = read(path)? else {
            return Ok(Diff {
                path: path.display().to_string(),
                before: None,
                after: None,
                backup_path: None,
            });
        };
        if let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) {
            self.strip_ours(hooks);
            if hooks.is_empty() {
                settings.remove("hooks");
            }
        }
        finish(path, Some(before), &settings, dry_run)
    }
}

/// Writes `settings` atomically, backing up the original before the first edit.
pub(crate) fn finish(
    path: &Path,
    before: Option<String>,
    settings: &Map<String, Value>,
    dry_run: bool,
) -> Result<Diff> {
    let after = serde_json::to_string_pretty(settings)? + "\n";
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
    write_atomic(path, diff.after.as_deref().unwrap_or_default())?;
    Ok(diff)
}

pub(crate) fn write_atomic(path: &Path, text: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("agentscope-tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Quotes `path` for the shell a harness runs hook commands with: POSIX
/// single quotes, or double quotes for `cmd.exe` and PowerShell.
pub(crate) fn shell_quote(path: &Path, windows: bool) -> String {
    let p = path.display().to_string();
    if windows {
        format!("\"{p}\"")
    } else {
        format!("'{}'", p.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_shim_in_paths_and_shell_lines() {
        for ours in [
            "/Applications/Agent Scope.app/Contents/MacOS/agentscope-hook",
            r"C:\Apps\agentscope-hook.exe",
            "'/opt/agent scope/agentscope-hook' --harness codex --event Stop",
            r#""C:\Program Files\AgentScope\agentscope-hook.exe" --harness cursor"#,
            "agentscope-hook --harness cursor --event stop",
        ] {
            assert!(runs_shim(ours), "{ours}");
        }
        for theirs in [
            "~/bin/audit.sh",
            "/usr/bin/agentscope-hooks",
            "my-agentscope-hook --x",
            "echo agentscope-hook-like",
        ] {
            assert!(!runs_shim(theirs), "{theirs}");
        }
    }
}
