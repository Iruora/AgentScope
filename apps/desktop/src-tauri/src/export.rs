//! Exports one session as Markdown, to read or paste, or as JSON, to keep or
//! process. Every string goes through redaction first.

use std::collections::HashMap;
use std::fmt::Write as _;

use agentscope_core::redact::{redact_json, redact_text, truncate};
use agentscope_core::{Agent, AgentEvent, EventKind, Session, SessionLabel};
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Markdown,
    Json,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Markdown => "md",
            ExportFormat::Json => "json",
        }
    }
}

/// Events included in an export, newest last.
pub const MAX_EVENTS: u32 = 5000;
/// Longest prompt or error kept in the Markdown timeline.
const MAX_LINE: usize = 600;

pub struct Export<'a> {
    pub session: &'a Session,
    pub agents: &'a [Agent],
    pub events: &'a [AgentEvent],
    pub label: Option<&'a SessionLabel>,
    pub cost: Option<f64>,
}

/// A file name from the title, safe on every OS.
pub fn file_name(export: &Export, format: ExportFormat) -> String {
    let title = title(export);
    let mut name: String = title
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
        .chars()
        .take(60)
        .collect();
    if name.is_empty() {
        name = "session".into();
    }
    let day = export
        .session
        .started_at
        .with_timezone(&Local)
        .format("%Y-%m-%d");
    format!("{day}-{name}.{}", format.extension())
}

fn title(export: &Export) -> String {
    export
        .label
        .and_then(|l| l.name.clone())
        .or_else(|| export.session.title.clone())
        .unwrap_or_else(|| "Untitled session".into())
}

fn local(at: DateTime<Utc>) -> String {
    at.with_timezone(&Local)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

fn duration(from: DateTime<Utc>, to: DateTime<Utc>) -> String {
    let minutes = (to - from).num_minutes().max(0);
    if minutes < 60 {
        format!("{minutes} min")
    } else {
        format!("{} h {} min", minutes / 60, minutes % 60)
    }
}

/// One line, safe inside Markdown inline code.
fn code(text: &str) -> String {
    let line = truncate(&redact_text(text), MAX_LINE)
        .replace(['\n', '\r'], " ")
        .replace('`', "'");
    format!("`{line}`")
}

/// One line of prose, redacted.
fn prose(text: &str) -> String {
    truncate(&redact_text(text), MAX_LINE).replace(['\n', '\r'], " ")
}

pub fn markdown(export: &Export) -> String {
    let s = export.session;
    let mut out = String::new();
    let _ = writeln!(out, "# {}\n", prose(&title(export)));
    if let Some(label) = export.label {
        if !label.tags.is_empty() {
            let tags: Vec<_> = label.tags.iter().map(|t| format!("#{t}")).collect();
            let _ = writeln!(out, "{}\n", tags.join(" "));
        }
        if let Some(note) = &label.note {
            for line in redact_text(note).lines() {
                let _ = writeln!(out, "> {line}");
            }
            out.push('\n');
        }
    }

    let mut facts: Vec<(&str, String)> = vec![
        ("Harness", s.harness.display_name().into()),
        ("Folder", code(&s.cwd)),
    ];
    if let Some(repo) = &s.repo {
        if let Some(branch) = &repo.branch {
            facts.push(("Branch", code(branch)));
        }
    }
    if let Some(model) = &s.model {
        facts.push(("Model", model.clone()));
    }
    facts.push(("Started", local(s.started_at)));
    match s.ended_at {
        Some(end) => facts.push((
            "Ended",
            format!("{} ({})", local(end), duration(s.started_at, end)),
        )),
        None => facts.push(("Ended", "Still running".into())),
    }
    if let Some(t) = &s.tokens {
        let cached = t
            .cached
            .map(|c| format!(", {c} cached"))
            .unwrap_or_default();
        facts.push((
            "Tokens",
            format!("{} in, {} out{cached}", t.input, t.output),
        ));
    }
    if let Some(cost) = export.cost {
        facts.push(("Estimated cost", format!("~${cost:.2} at API list prices")));
    }
    for (k, v) in facts {
        let _ = writeln!(out, "- **{k}:** {v}");
    }

    let subagents: Vec<_> = export
        .agents
        .iter()
        .filter(|a| a.parent_agent_id.is_some())
        .collect();
    if !subagents.is_empty() {
        let _ = writeln!(out, "\n## Subagents\n");
        for a in subagents {
            let what = a
                .description
                .as_deref()
                .or(a.agent_type.as_deref())
                .unwrap_or("Subagent");
            let result = a
                .result
                .map(|r| format!(" ({})", format!("{r:?}").to_lowercase()))
                .unwrap_or_default();
            let _ = writeln!(out, "- {}{result}", prose(what));
        }
    }

    if !s.files.is_empty() {
        let root = s.repo.as_ref().map(|r| r.root.as_str()).unwrap_or(&s.cwd);
        let _ = writeln!(out, "\n## Files changed\n");
        for f in &s.files {
            let path = f
                .path
                .strip_prefix(root)
                .map(|p| p.trim_start_matches(['/', '\\']))
                .unwrap_or(&f.path);
            let edits = if f.edits == 1 { "edit" } else { "edits" };
            let _ = writeln!(out, "- {} ({} {edits})", code(path), f.edits);
        }
    }

    let names: HashMap<&str, String> = export
        .agents
        .iter()
        .filter(|a| a.parent_agent_id.is_some())
        .map(|a| {
            let native = a.id.rsplit('/').next().unwrap_or(&a.id);
            let name = a
                .description
                .as_deref()
                .or(a.agent_type.as_deref())
                .unwrap_or("subagent");
            (native, prose(name))
        })
        .collect();
    let lines: Vec<String> = export
        .events
        .iter()
        .filter_map(|ev| {
            let text = describe(&ev.kind)?;
            let who = ev
                .agent
                .as_deref()
                .and_then(|a| names.get(a))
                .map(|n| format!(" _[{n}]_"))
                .unwrap_or_default();
            let at = ev.at.with_timezone(&Local).format("%H:%M:%S");
            Some(format!("- {at}{who} {text}"))
        })
        .collect();
    if !lines.is_empty() {
        let _ = writeln!(out, "\n## Timeline\n");
        for line in lines {
            let _ = writeln!(out, "{line}");
        }
    }
    let _ = writeln!(
        out,
        "\n---\nExported from AgentScope on {}. Secrets that look like keys or tokens were replaced with [redacted].",
        local(Utc::now())
    );
    out
}

fn describe(kind: &EventKind) -> Option<String> {
    Some(match kind {
        EventKind::SessionStarted { .. } => "Session started".into(),
        EventKind::SessionEnded => "Session ended".into(),
        EventKind::PromptSubmitted { prompt } => format!("**Prompt:** {}", prose(prompt)),
        EventKind::AgentStarted {
            description,
            agent_type,
            ..
        } => format!(
            "Subagent started: {}",
            prose(
                description
                    .as_deref()
                    .or(agent_type.as_deref())
                    .unwrap_or("subagent")
            )
        ),
        EventKind::AgentStopped { result } => {
            format!("Agent stopped ({})", format!("{result:?}").to_lowercase())
        }
        EventKind::ToolStarted { summary, .. } => code(summary),
        EventKind::ToolFailed { error } => match error {
            Some(e) => format!("Failed: {}", prose(e)),
            None => "Failed".into(),
        },
        EventKind::ApprovalRequested { summary, .. } => {
            format!("Asked for approval: {}", code(summary))
        }
        EventKind::ApprovalResolved { resolution, .. } => {
            format!("Approval {}", format!("{resolution:?}").to_lowercase())
        }
        EventKind::WaitingForInput => "Waiting for you".into(),
        EventKind::TurnCompleted => "Turn finished".into(),
        EventKind::Compacted => "Context compacted".into(),
        _ => return None,
    })
}

/// Everything AgentScope knows about the session, redacted.
pub fn json(export: &Export) -> Result<String, serde_json::Error> {
    let value = serde_json::json!({
        "format": "agentscope.session.v1",
        "exportedAt": Utc::now(),
        "session": export.session,
        "label": export.label,
        "estimatedCostUsd": export.cost,
        "agents": export.agents,
        "events": export.events,
    });
    serde_json::to_string_pretty(&redact_json(&value, 20_000))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentscope_core::{EventSource, HarnessId};

    fn session() -> Session {
        let at = Utc::now();
        serde_json::from_value(serde_json::json!({
            "id": "codex:s1", "harness": "codex", "nativeId": "s1", "title": "Fix: the `login` flow!",
            "cwd": "/w/app", "repo": {"root": "/w/app", "branch": "main"}, "model": "gpt-5.3-codex",
            "startedAt": at, "lastEventAt": at, "endedAt": at, "state": "ended", "pid": null,
            "terminal": null, "transcriptPath": null, "tokens": {"input": 1200, "output": 300, "cached": null},
            "rootAgentId": "codex:s1/main",
            "files": [{"path": "/w/app/src/login.ts", "edits": 2, "firstAt": at, "lastAt": at, "agentId": "codex:s1/main"}],
        }))
        .unwrap()
    }

    fn ev(kind: EventKind) -> AgentEvent {
        AgentEvent {
            source: EventSource::Hook,
            at: Utc::now(),
            harness: HarnessId::Codex,
            session: "s1".into(),
            agent: None,
            dedupe_key: None,
            kind,
        }
    }

    #[test]
    fn markdown_and_json_are_redacted() {
        let session = session();
        let secret = "sk-abcdefghijklmnopqrstuvwxyz123456";
        let events = vec![
            ev(EventKind::PromptSubmitted {
                prompt: format!("use key {secret}"),
            }),
            ev(EventKind::ToolStarted {
                tool: "Bash".into(),
                summary: "Bash: npm test `--watch`".into(),
                files: vec![],
            }),
            ev(EventKind::ToolFinished),
        ];
        let label = SessionLabel {
            session_id: session.id.clone(),
            tags: vec!["auth".into()],
            note: Some(format!("token {secret}")),
            ..Default::default()
        };
        let export = Export {
            session: &session,
            agents: &[],
            events: &events,
            label: Some(&label),
            cost: Some(0.123),
        };
        let md = markdown(&export);
        assert!(!md.contains(secret), "{md}");
        assert!(md.contains("# Fix: the `login` flow!"));
        assert!(md.contains("#auth"));
        assert!(md.contains("`src/login.ts` (2 edits)"));
        assert!(md.contains("`Bash: npm test '--watch'`"));
        assert!(md.contains("~$0.12"));
        assert_eq!(md.matches("\n- ").count(), 8 + 1 + 2, "{md}");

        let js = json(&export).unwrap();
        assert!(!js.contains(secret));
        let parsed: serde_json::Value = serde_json::from_str(&js).unwrap();
        assert_eq!(parsed["format"], "agentscope.session.v1");
        assert_eq!(parsed["events"].as_array().unwrap().len(), 3);

        let name = file_name(&export, ExportFormat::Markdown);
        assert!(name.ends_with("-fix-the-login-flow.md"), "{name}");
    }
}
