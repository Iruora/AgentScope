//! Best-effort removal of secrets from text shown in the UI or stored on disk.
//!
//! Tool inputs can carry API keys (`curl -H "Authorization: ..."`,
//! `export OPENAI_API_KEY=...`). Everything that leaves an adapter as a summary
//! or approval detail goes through here first.

const REDACTED: &str = "[redacted]";

/// Prefixes of well-known token formats. A match must be followed by at
/// least [`MIN_SECRET_TAIL`] token characters.
const TOKEN_PREFIXES: &[&str] = &[
    "sk-",
    "ghp_",
    "gho_",
    "ghs_",
    "ghu_",
    "ghr_",
    "github_pat_",
    "glpat-",
    "xoxb-",
    "xoxp-",
    "xoxa-",
    "AKIA",
    "ASIA",
    "AIza",
    "npm_",
    "pypi-",
];
const MIN_SECRET_TAIL: usize = 16;

/// Words that make the value assigned to a name secret: `API_KEY=...`,
/// `"password": ...`, `Authorization: Bearer ...`.
const SECRET_NAMES: &[&str] = &[
    "token",
    "secret",
    "password",
    "passwd",
    "api_key",
    "apikey",
    "api-key",
    "authorization",
    "private_key",
    "credential",
];

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '+' | '=')
}

fn is_secret_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    SECRET_NAMES.iter().any(|w| lower.contains(w))
}

/// Replaces secrets-looking substrings of `text`.
pub fn redact_text(text: &str) -> String {
    let text = redact_assignments(text);
    redact_tokens(&text)
}

/// `NAME=value`, `NAME: value`, and `--name value` where NAME looks secret.
fn redact_assignments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find(['=', ':']) {
        let (head, tail) = rest.split_at(pos);
        let name_start = head
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '"' | '\'')))
            .map_or(0, |i| i + 1);
        let name = head[name_start..].trim_matches(['"', '\'']);
        out.push_str(head);
        let sep = &tail[..1];
        out.push_str(sep);
        let after = &tail[1..];
        if !name.is_empty() && is_secret_name(name) {
            // Skip spaces, an optional quote, and an optional auth scheme.
            let trimmed = after.trim_start_matches([' ', '"', '\'']);
            let skipped = after.len() - trimmed.len();
            let mut value = trimmed;
            let mut scheme = "";
            for s in ["Bearer ", "Basic ", "token "] {
                if let Some(v) = trimmed.strip_prefix(s) {
                    scheme = s;
                    value = v;
                }
            }
            let len = value
                .find(|c: char| !is_token_char(c))
                .unwrap_or(value.len());
            if len > 0 {
                out.push_str(&after[..skipped]);
                out.push_str(scheme);
                out.push_str(REDACTED);
                rest = &value[len..];
                continue;
            }
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

fn redact_tokens(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let bytes = text.as_bytes();
    while i < text.len() {
        let at_boundary = i == 0 || !is_token_char(text[..i].chars().next_back().unwrap_or(' '));
        if at_boundary {
            if let Some(prefix) = TOKEN_PREFIXES.iter().find(|p| text[i..].starts_with(**p)) {
                let tail = &text[i + prefix.len()..];
                let len = tail.find(|c: char| !is_token_char(c)).unwrap_or(tail.len());
                if len >= MIN_SECRET_TAIL {
                    out.push_str(prefix);
                    out.push_str(REDACTED);
                    i += prefix.len() + len;
                    continue;
                }
            }
            if text[i..].starts_with("-----BEGIN") {
                out.push_str(REDACTED);
                i = text[i..]
                    .find("-----END")
                    .and_then(|end| {
                        let after = i + end + "-----END".len();
                        text[after..].find("-----").map(|e| after + e + 5)
                    })
                    .unwrap_or(text.len());
                continue;
            }
        }
        let ch = text[i..].chars().next().expect("i is a char boundary");
        out.push(ch);
        i += ch.len_utf8();
        debug_assert!(i <= bytes.len());
    }
    out
}

/// Redacts string values (and values under secret-looking keys) in a JSON
/// value, and cuts very long strings.
pub fn redact_json(value: &serde_json::Value, max_string: usize) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::String(s) => Value::String(truncate(&redact_text(s), max_string)),
        Value::Array(items) => {
            Value::Array(items.iter().map(|v| redact_json(v, max_string)).collect())
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| {
                    let v = if is_secret_name(k) && !v.is_null() {
                        Value::String(REDACTED.into())
                    } else {
                        redact_json(v, max_string)
                    };
                    (k.clone(), v)
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Cuts `text` to at most `max` characters, marking the cut with `…`.
pub fn truncate(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_known_token_formats() {
        let key = format!("sk-ant-{}", "a".repeat(40));
        assert_eq!(
            redact_text(&format!("curl -d {key} x")),
            "curl -d sk-[redacted] x"
        );
        assert_eq!(
            redact_text(&format!("push ghp_{}", "Z9".repeat(18))),
            "push ghp_[redacted]"
        );
        // Short look-alikes and words are kept.
        assert_eq!(redact_text("task-runner sk-short"), "task-runner sk-short");
    }

    #[test]
    fn redacts_secret_assignments() {
        assert_eq!(
            redact_text("export OPENAI_API_KEY=abc123 && run"),
            "export OPENAI_API_KEY=[redacted] && run"
        );
        assert_eq!(
            redact_text(r#"curl -H "Authorization: Bearer abc.def" url"#),
            r#"curl -H "Authorization: Bearer [redacted]" url"#
        );
        assert_eq!(
            redact_text("password: hunter2, user: bob"),
            "password: [redacted], user: bob"
        );
        assert_eq!(redact_text("cd /a:b && x=1"), "cd /a:b && x=1");
    }

    #[test]
    fn redacts_private_keys() {
        let text = "key -----BEGIN RSA PRIVATE KEY-----\nabc\n-----END RSA PRIVATE KEY----- done";
        assert_eq!(redact_text(text), "key [redacted] done");
    }

    #[test]
    fn redacts_json_values_and_truncates() {
        let value = serde_json::json!({
            "command": "TOKEN=abc ls",
            "headers": { "x-api-key": "zzz" },
            "content": "x".repeat(30),
            "n": 3
        });
        assert_eq!(
            redact_json(&value, 20),
            serde_json::json!({
                "command": "TOKEN=[redacted] ls",
                "headers": { "x-api-key": "[redacted]" },
                "content": format!("{}…", "x".repeat(20)),
                "n": 3
            })
        );
    }

    #[test]
    fn truncates_on_char_boundaries() {
        assert_eq!(truncate("héllo", 2), "hé…");
        assert_eq!(truncate("hi", 2), "hi");
    }
}
