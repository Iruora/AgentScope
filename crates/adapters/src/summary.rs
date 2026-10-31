//! One-line, redacted descriptions of tool calls: "Bash: npm test",
//! "Edit src/app.ts". Shared by adapters so every harness reads the same.

use std::path::Path;

use agentscope_core::redact::{redact_text, truncate};
use serde_json::Value;

/// Longest summary shown on a card or in the approvals inbox.
pub const SUMMARY_MAX_CHARS: usize = 160;

/// Longest string kept inside an approval's full tool input.
pub const DETAIL_MAX_STRING: usize = 4000;

fn str_field<'a>(input: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|k| input.get(*k).and_then(Value::as_str))
        .filter(|s| !s.trim().is_empty())
}

/// `path` relative to `cwd` when it is inside it.
fn display_path(path: &str, cwd: &str) -> String {
    if cwd.is_empty() {
        return path.to_owned();
    }
    Path::new(path)
        .strip_prefix(cwd)
        .ok()
        .map(|p| p.display().to_string())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| path.to_owned())
}

fn first_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
}

/// Summarizes a tool call from its name and input. Knows the tool names of
/// every supported harness: Claude Code's `Bash`, Codex's `apply_patch`,
/// Copilot CLI's `view` and `create`, Cursor's `Shell`.
pub fn tool_summary(tool: &str, input: &Value, cwd: &str) -> String {
    let name = display_tool(tool);
    let text = match tool.to_ascii_lowercase().as_str() {
        "bash" | "bashoutput" | "powershell" | "shell" | "run_terminal_cmd" => {
            str_field(input, &["command", "cmd"]).map(|c| {
                let more = if c.trim().lines().nth(1).is_some() {
                    " …"
                } else {
                    ""
                };
                format!("{name}: {}{more}", first_line(c))
            })
        }
        // File tools read as "Edit src/app.ts".
        "read" | "write" | "edit" | "multiedit" | "notebookedit" | "notebookread" | "view"
        | "create" | "delete" | "str_replace_editor" | "read_file" | "edit_file" => str_field(
            input,
            &["file_path", "notebook_path", "path", "target_file"],
        )
        .map(|p| format!("{name} {}", display_path(p, cwd))),
        "apply_patch" => str_field(input, &["command", "input", "patch"])
            .and_then(patched_file)
            .map(|p| format!("Edit {}", display_path(p, cwd))),
        "glob" | "grep" | "rg" => {
            str_field(input, &["pattern", "query"]).map(|p| format!("{name}: {p}"))
        }
        "webfetch" | "web_fetch" => str_field(input, &["url"]).map(|u| format!("{name}: {u}")),
        "websearch" | "web_search" => str_field(input, &["query"]).map(|q| format!("{name}: {q}")),
        "task" | "agent" | "spawn_agent" => str_field(input, &["description", "prompt", "message"])
            .map(|d| format!("{name}: {}", first_line(d))),
        "skill" => str_field(input, &["skill", "command"]).map(|s| format!("{name}: {s}")),
        _ => None,
    };
    truncate(&redact_text(&text.unwrap_or(name)), SUMMARY_MAX_CHARS)
}

/// Absolute paths a tool call writes: edits, new files, deletions and every
/// file in an `apply_patch` envelope. Reads and shell commands write nothing
/// we can see, so they return none.
pub fn tool_files(tool: &str, input: &Value, cwd: &str) -> Vec<String> {
    // Joined as strings with the cwd's own separator, so a path reads the
    // same whichever OS AgentScope runs on.
    let absolute = |p: &str| -> String {
        let p = p.trim();
        let rooted =
            p.starts_with(['/', '\\']) || p.get(1..3).is_some_and(|s| s == ":\\" || s == ":/");
        if rooted || cwd.is_empty() {
            return p.to_owned();
        }
        let sep = if cwd.contains('\\') && !cwd.contains('/') {
            '\\'
        } else {
            '/'
        };
        let rel = p
            .strip_prefix("./")
            .or_else(|| p.strip_prefix(".\\"))
            .unwrap_or(p);
        format!("{}{sep}{rel}", cwd.trim_end_matches(['/', '\\']))
    };
    match tool.to_ascii_lowercase().as_str() {
        "write" | "edit" | "multiedit" | "notebookedit" | "create" | "delete" | "edit_file"
        | "write_file" | "search_replace" | "delete_file" => str_field(
            input,
            &["file_path", "notebook_path", "path", "target_file"],
        )
        .map(|p| vec![absolute(p)])
        .unwrap_or_default(),
        // Copilot's editor tool also views files.
        "str_replace_editor" | "edit_tool" => {
            if str_field(input, &["command"]) == Some("view") {
                return Vec::new();
            }
            str_field(input, &["path", "file_path"])
                .map(|p| vec![absolute(p)])
                .unwrap_or_default()
        }
        "apply_patch" => str_field(input, &["command", "input", "patch"])
            .map(|patch| patched_files(patch).map(absolute).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Every file an `apply_patch` envelope touches.
fn patched_files(patch: &str) -> impl Iterator<Item = &str> {
    patch.lines().filter_map(|l| {
        [
            "*** Update File: ",
            "*** Add File: ",
            "*** Delete File: ",
            "*** Move to: ",
        ]
        .iter()
        .find_map(|p| l.trim().strip_prefix(p))
        .map(str::trim)
    })
}

/// The first file an `apply_patch` envelope touches.
fn patched_file(patch: &str) -> Option<&str> {
    patch.lines().find_map(|l| {
        ["*** Update File: ", "*** Add File: ", "*** Delete File: "]
            .iter()
            .find_map(|p| l.trim().strip_prefix(p))
            .map(str::trim)
    })
}

/// `mcp__github__create_issue` reads as `github · create_issue`.
pub fn display_tool(tool: &str) -> String {
    match tool.strip_prefix("mcp__").and_then(|t| t.split_once("__")) {
        Some((server, name)) => format!("{server} · {name}"),
        None => tool.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn finds_written_files() {
        let cwd = "/repo";
        assert_eq!(
            tool_files("Edit", &json!({ "file_path": "/repo/src/app.ts" }), cwd),
            ["/repo/src/app.ts"]
        );
        assert_eq!(
            tool_files("create", &json!({ "path": "./notes.md" }), cwd),
            ["/repo/notes.md"]
        );
        assert!(tool_files("Read", &json!({ "file_path": "/repo/a" }), cwd).is_empty());
        assert!(tool_files("Bash", &json!({ "command": "rm a" }), cwd).is_empty());
        assert!(tool_files(
            "str_replace_editor",
            &json!({ "command": "view", "path": "/repo/a" }),
            cwd
        )
        .is_empty());
        assert_eq!(
            tool_files(
                "apply_patch",
                &json!({ "command": "*** Begin Patch\n*** Update File: src/a.rs\n@@\n*** Add File: /repo/b.rs\n" }),
                cwd
            ),
            ["/repo/src/a.rs", "/repo/b.rs"]
        );
        assert_eq!(
            tool_files("write", &json!({ "path": ".\\a.txt" }), "C:\\work\\"),
            ["C:\\work\\a.txt"]
        );
        assert_eq!(
            tool_files("write", &json!({ "path": "D:\\b.txt" }), "C:\\work"),
            ["D:\\b.txt"]
        );
    }

    #[test]
    fn summarizes_common_tools() {
        let cwd = "/repo";
        assert_eq!(
            tool_summary("Bash", &json!({ "command": "npm test" }), cwd),
            "Bash: npm test"
        );
        assert_eq!(
            tool_summary("Bash", &json!({ "command": "cd x\nmake" }), cwd),
            "Bash: cd x …"
        );
        assert_eq!(
            tool_summary("Edit", &json!({ "file_path": "/repo/src/app.ts" }), cwd),
            "Edit src/app.ts"
        );
        assert_eq!(
            tool_summary("Read", &json!({ "file_path": "/etc/hosts" }), cwd),
            "Read /etc/hosts"
        );
        assert_eq!(
            tool_summary("Task", &json!({ "description": "Find the loader" }), cwd),
            "Task: Find the loader"
        );
        assert_eq!(
            tool_summary("mcp__github__create_issue", &json!({}), cwd),
            "github · create_issue"
        );
        assert_eq!(
            tool_summary(
                "apply_patch",
                &json!({ "command": "*** Begin Patch\n*** Update File: /repo/src/a.rs\n@@" }),
                cwd
            ),
            "Edit src/a.rs"
        );
        assert_eq!(
            tool_summary("view", &json!({ "path": "/repo/README.md" }), cwd),
            "view README.md"
        );
        assert_eq!(
            tool_summary("Shell", &json!({ "command": "ls" }), cwd),
            "Shell: ls"
        );
    }

    #[test]
    fn redacts_and_truncates() {
        let s = tool_summary("Bash", &json!({ "command": "API_KEY=abc curl x" }), "");
        assert_eq!(s, "Bash: API_KEY=[redacted] curl x");
        let long = tool_summary("Bash", &json!({ "command": "x".repeat(500) }), "");
        assert_eq!(long.chars().count(), SUMMARY_MAX_CHARS + 1);
    }
}
