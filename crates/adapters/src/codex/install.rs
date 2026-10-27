//! Adds and removes AgentScope's entries in `~/.codex/hooks.json`.
//!
//! Each event gets its own matcher group with one command handler:
//!
//! ```json
//! { "hooks": [{ "type": "command", "async": true, "timeout": 3,
//!   "command": "'/…/agentscope-hook' --harness codex --event Stop" }] }
//! ```
//!
//! Codex runs commands through a shell (`sh -lc`, or `cmd /C` on Windows), so
//! the shim path is quoted. `async` runs the hook in the background; Codex
//! runs `SessionEnd` hooks synchronously anyway, so that one is registered
//! without it to avoid a warning. Codex caps `SessionEnd` at 3 seconds, and
//! the shim gives up after 300 ms, so every event uses that timeout.
//!
//! Codex only runs a user hook after it is trusted in `/hooks`, which stores
//! the hash of the hook's normalized definition in `config.toml` under
//! `[hooks.state."<hooks.json>:<event>:<group>:<handler>"] trusted_hash`. The
//! status is `NeedsTrust` until every one of our hooks has a matching hash.

use std::collections::HashSet;
use std::path::Path;

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::hooks::EVENTS;
use crate::config::{shell_quote, HookConfig};
use crate::{Diff, HookPlan, HookStatus, Result};

const HOOK_TIMEOUT_SECS: u64 = 3;

fn is_async(event: &str) -> bool {
    event != "SessionEnd"
}

fn command(shim: &Path, event: &str) -> String {
    format!(
        "{} --harness codex --event {event}",
        shell_quote(shim, cfg!(windows))
    )
}

fn hook_entry(shim: &Path, event: &str) -> Value {
    json!({
        "type": "command",
        "command": command(shim, event),
        "timeout": HOOK_TIMEOUT_SECS,
        "async": is_async(event),
    })
}

const CONFIG: HookConfig<'static> = HookConfig {
    events: EVENTS,
    entry: &hook_entry,
    grouped: true,
    required: &[],
};

/// Codex's key for an event in hook state, e.g. `pre_tool_use`.
fn event_label(event: &str) -> String {
    let mut out = String::new();
    for (i, c) in event.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// Rebuilds a value with object keys in sorted order.
fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            Value::Object(
                keys.into_iter()
                    .map(|k| (k.clone(), sorted(&map[k])))
                    .collect::<Map<String, Value>>(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

/// The hash `/hooks` stores when the user trusts our hook for `event`: the
/// SHA-256 of the key-sorted, compact JSON of the normalized definition
/// (codex-rs `hooks/src/engine/discovery.rs`, `hook_hash`).
pub fn trust_hash(shim: &Path, event: &str) -> String {
    let identity = json!({
        "event_name": event_label(event),
        "hooks": [hook_entry(shim, event)],
    });
    let bytes = serde_json::to_vec(&sorted(&identity)).unwrap_or_default();
    let digest = Sha256::digest(bytes);
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

/// Every `trusted_hash` in `config.toml`.
fn trusted_hashes(config_toml: &Path) -> HashSet<String> {
    let Ok(text) = std::fs::read_to_string(config_toml) else {
        return HashSet::new();
    };
    let Ok(config) = text.parse::<toml::Table>() else {
        return HashSet::new();
    };
    config
        .get("hooks")
        .and_then(|h| h.get("state"))
        .and_then(toml::Value::as_table)
        .into_iter()
        .flatten()
        .filter_map(|(_, state)| state.get("trusted_hash")?.as_str().map(str::to_owned))
        .collect()
}

pub fn status(home: &Path, shim: &Path) -> HookStatus {
    let status = CONFIG.status(&home.join("hooks.json"), shim);
    if status != HookStatus::Installed {
        return status;
    }
    let trusted = trusted_hashes(&home.join("config.toml"));
    if EVENTS
        .iter()
        .all(|e| trusted.contains(&trust_hash(shim, e)))
    {
        HookStatus::Installed
    } else {
        HookStatus::NeedsTrust
    }
}

pub fn install(home: &Path, plan: &HookPlan) -> Result<Diff> {
    CONFIG.install(&home.join("hooks.json"), &plan.shim_path, plan.dry_run)
}

pub fn uninstall(home: &Path, dry_run: bool) -> Result<Diff> {
    CONFIG.uninstall(&home.join("hooks.json"), dry_run)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn plan(shim: &str) -> HookPlan {
        HookPlan {
            shim_path: PathBuf::from(shim),
            dry_run: false,
        }
    }

    #[test]
    fn labels_match_codex() {
        assert_eq!(event_label("PreToolUse"), "pre_tool_use");
        assert_eq!(event_label("SessionStart"), "session_start");
        assert_eq!(event_label("Stop"), "stop");
    }

    #[test]
    fn hash_is_over_the_sorted_compact_identity() {
        // Computed independently: sha256 of
        // {"event_name":"stop","hooks":[{"async":true,"command":"'/x/agentscope-hook' --harness codex --event Stop","timeout":3,"type":"command"}]}
        let expected = {
            let text = r#"{"event_name":"stop","hooks":[{"async":true,"command":"'/x/agentscope-hook' --harness codex --event Stop","timeout":3,"type":"command"}]}"#;
            let hex: String = Sha256::digest(text.as_bytes())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            format!("sha256:{hex}")
        };
        if !cfg!(windows) {
            assert_eq!(
                trust_hash(Path::new("/x/agentscope-hook"), "Stop"),
                expected
            );
        }
    }

    #[test]
    fn needs_trust_until_every_hook_is_trusted() {
        let home = tempfile::tempdir().unwrap();
        let shim = Path::new("/opt/agent scope/agentscope-hook");
        fs::write(
            home.path().join("hooks.json"),
            r#"{ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "notify-send done" }] }] } }"#,
        )
        .unwrap();
        assert_eq!(status(home.path(), shim), HookStatus::NotInstalled);

        let diff = install(home.path(), &plan("/opt/agent scope/agentscope-hook")).unwrap();
        assert!(diff.backup_path.is_some());
        assert_eq!(status(home.path(), shim), HookStatus::NeedsTrust);
        let written: Value =
            serde_json::from_str(&fs::read_to_string(home.path().join("hooks.json")).unwrap())
                .unwrap();
        assert_eq!(written["hooks"]["Stop"].as_array().unwrap().len(), 2);
        let ours = &written["hooks"]["SessionEnd"][0]["hooks"][0];
        assert_eq!(ours["async"], false);
        if !cfg!(windows) {
            assert_eq!(
                ours["command"],
                "'/opt/agent scope/agentscope-hook' --harness codex --event SessionEnd"
            );
        }

        // What `/hooks` writes after the user trusts them all.
        // TOML escapes backslashes in Windows paths, as Codex's writer does.
        let path = home
            .path()
            .join("hooks.json")
            .display()
            .to_string()
            .replace('\\', "\\\\");
        let mut config = String::from("model = \"gpt-5.5\"\n");
        for (i, event) in EVENTS.iter().enumerate() {
            config.push_str(&format!(
                "\n[hooks.state.\"{path}:{}:{i}:0\"]\ntrusted_hash = \"{}\"\n",
                event_label(event),
                trust_hash(shim, event)
            ));
        }
        fs::write(home.path().join("config.toml"), config).unwrap();
        assert_eq!(status(home.path(), shim), HookStatus::Installed);

        // A moved app is outdated, and its new entries need trust again.
        assert_eq!(
            status(home.path(), Path::new("/new/agentscope-hook")),
            HookStatus::Outdated
        );

        uninstall(home.path(), false).unwrap();
        let restored: Value =
            serde_json::from_str(&fs::read_to_string(home.path().join("hooks.json")).unwrap())
                .unwrap();
        assert_eq!(restored["hooks"]["Stop"].as_array().unwrap().len(), 1);
        assert!(restored["hooks"].get("PreToolUse").is_none());
    }
}
