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

/// Summarizes a tool call from its name and input.
pub fn tool_summary(tool: &str, input: &Value, cwd: &str) -> String {
    let name = display_tool(tool);
    let text = match tool {
        "Bash" | "BashOutput" | "PowerShell" | "shell" | "Shell" => {
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
        "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit" | "NotebookRead" => {
            str_field(input, &["file_path", "notebook_path", "path"])
                .map(|p| format!("{name} {}", display_path(p, cwd)))
        }
        "Glob" | "Grep" => str_field(input, &["pattern"]).map(|p| format!("{name}: {p}")),
        "WebFetch" => str_field(input, &["url"]).map(|u| format!("{name}: {u}")),
        "WebSearch" => str_field(input, &["query"]).map(|q| format!("{name}: {q}")),
        "Task" | "Agent" => str_field(input, &["description", "prompt"])
            .map(|d| format!("{name}: {}", first_line(d))),
        "Skill" => str_field(input, &["skill", "command"]).map(|s| format!("{name}: {s}")),
        _ => None,
    };
    truncate(&redact_text(&text.unwrap_or(name)), SUMMARY_MAX_CHARS)
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
    }

    #[test]
    fn redacts_and_truncates() {
        let s = tool_summary("Bash", &json!({ "command": "API_KEY=abc curl x" }), "");
        assert_eq!(s, "Bash: API_KEY=[redacted] curl x");
        let long = tool_summary("Bash", &json!({ "command": "x".repeat(500) }), "");
        assert_eq!(long.chars().count(), SUMMARY_MAX_CHARS + 1);
    }
}
