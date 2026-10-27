//! Replays recorded Codex 0.165 hook payloads and rollouts from `fixtures/`.

use std::path::{Path, PathBuf};

use agentscope_adapters::{Adapter, Codex, TranscriptFile};
use agentscope_core::{
    AgentEvent, AgentKind, AgentState, Confidence, EventKind, Graph, HarnessId, RawHookEvent,
};

const SESSION: &str = "019a2b3c-4d5e-7f60-8a9b-0c1d2e3f4a5b";
const SUBAGENT: &str = "019a2b3c-4d5e-7f60-8a9b-0c1d2e3f4a77";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/codex/0.165")
}

fn hook(name: &str, at: u64) -> RawHookEvent {
    let payload = std::fs::read(fixtures().join("hooks").join(format!("{name}.json"))).unwrap();
    RawHookEvent {
        v: 1,
        harness: HarnessId::Codex,
        event: name.split('.').next().unwrap().into(),
        received_at: 1_800_000_000_000 + at,
        pid: Some(11),
        ppid: Some(10),
        payload: serde_json::from_slice(&payload).unwrap(),
    }
}

fn adapter() -> Codex {
    Codex::with_home(fixtures())
}

struct Replay {
    graph: Graph,
    t: u64,
}

impl Replay {
    fn new() -> Self {
        Self {
            graph: Graph::new(),
            t: 0,
        }
    }

    fn hook(&mut self, name: &str) -> Vec<EventKind> {
        self.t += 1000;
        let events = adapter().normalize_hook(&hook(name, self.t));
        for ev in &events {
            self.graph.apply(ev);
        }
        events.into_iter().map(|e| e.kind).collect()
    }

    fn state(&self) -> AgentState {
        self.graph
            .session(&format!("codex:{SESSION}"))
            .unwrap()
            .state
    }
}

#[test]
fn hook_session_walkthrough() {
    let mut r = Replay::new();
    r.hook("SessionStart");
    let session = r.graph.session(&format!("codex:{SESSION}")).unwrap();
    assert_eq!(session.cwd, "/work/api");
    assert_eq!(session.model.as_deref(), Some("gpt-5.5"));

    r.hook("UserPromptSubmit");
    assert_eq!(r.state(), AgentState::Thinking);
    r.hook("PreToolUse");
    assert_eq!(r.state(), AgentState::RunningTool);

    let kinds = r.hook("PermissionRequest");
    let EventKind::ApprovalRequested {
        summary,
        detail,
        confidence,
        ..
    } = &kinds[0]
    else {
        panic!("expected an approval, got {kinds:?}");
    };
    assert_eq!(*confidence, Confidence::Confirmed);
    assert_eq!(
        summary,
        "Bash: DATABASE_URL=postgres://admin:[redacted]@db/prod sqlx migrate run"
    );
    assert!(!detail.as_ref().unwrap().to_string().contains("hunter2"));
    assert_eq!(r.state(), AgentState::WaitingApproval);

    r.hook("PostToolUse");
    assert!(r.graph.snapshot().approvals.is_empty());

    r.hook("SubagentStart");
    r.hook("PreToolUse.subagent");
    let snap = r.graph.snapshot();
    let sub = snap
        .agents
        .iter()
        .find(|a| a.kind == AgentKind::Subagent)
        .unwrap();
    assert_eq!(sub.id, format!("codex:{SESSION}/{SUBAGENT}"));
    assert_eq!(sub.agent_type.as_deref(), Some("explorer"));
    assert_eq!(
        sub.current_activity.as_ref().unwrap().summary,
        "Bash: rg -n 'fn login' src"
    );
    r.hook("SubagentStop");
    r.hook("Stop");
    assert_eq!(r.state(), AgentState::WaitingInput);
    r.hook("SessionEnd");
    assert_eq!(r.state(), AgentState::Ended);
}

#[test]
fn every_fixture_payload_is_understood() {
    for entry in std::fs::read_dir(fixtures().join("hooks")).unwrap() {
        let name = entry.unwrap().path();
        let name = name.file_stem().unwrap().to_str().unwrap().to_owned();
        let events = adapter().normalize_hook(&hook(&name, 0));
        assert!(!events.is_empty(), "{name} produced no events");
        for ev in events {
            assert_eq!(ev.session, SESSION, "{name}");
        }
    }
}

fn rollout(name: &str) -> Vec<AgentEvent> {
    let path = fixtures().join("sessions/2026/10/27").join(name);
    let bytes = std::fs::read(&path).unwrap();
    adapter().parse_transcript_chunk(&TranscriptFile { path, offset: 0 }, &bytes)
}

fn main_rollout() -> Vec<AgentEvent> {
    rollout(&format!("rollout-2026-10-27T10-00-00-{SESSION}.jsonl"))
}

#[test]
fn rollouts_rebuild_the_session_and_its_subagent() {
    let mut graph = Graph::new();
    let sub = rollout(&format!("rollout-2026-10-27T10-00-03-{SUBAGENT}.jsonl"));
    for ev in main_rollout().iter().chain(sub.iter()) {
        graph.apply(ev);
    }
    let snap = graph.snapshot();
    let session = &snap.sessions[0];
    assert_eq!(session.cwd, "/work/api");
    assert_eq!(session.model.as_deref(), Some("gpt-5.5"));
    assert_eq!(
        session.title.as_deref(),
        Some("Add rate limiting to the login endpoint"),
        "injected context is not a prompt"
    );
    assert_eq!(session.state, AgentState::WaitingInput);
    let tokens = session.tokens.unwrap();
    assert_eq!(
        (tokens.input, tokens.output, tokens.cached),
        (4000, 900, Some(8000))
    );

    let sub = snap
        .agents
        .iter()
        .find(|a| a.kind == AgentKind::Subagent)
        .unwrap();
    assert_eq!(sub.agent_type.as_deref(), Some("explorer"));
    assert_eq!(sub.description.as_deref(), Some("Find the login handler"));
    assert_eq!(sub.state, AgentState::Ended);
}

#[test]
fn rollout_tools_read_like_hook_tools() {
    let summaries: Vec<String> = main_rollout()
        .into_iter()
        .filter_map(|e| match e.kind {
            EventKind::ToolStarted { summary, .. } => Some(summary),
            _ => None,
        })
        .collect();
    assert_eq!(summaries, ["Bash: cargo test -p api", "Edit src/auth.rs"]);
}

#[test]
fn later_chunks_learn_their_owner_from_the_first_line() {
    let path = fixtures()
        .join("sessions/2026/10/27")
        .join(format!("rollout-2026-10-27T10-00-03-{SUBAGENT}.jsonl"));
    let bytes = std::fs::read(&path).unwrap();
    let cut = bytes.iter().position(|&b| b == b'\n').unwrap() + 1;
    let events = adapter().parse_transcript_chunk(
        &TranscriptFile {
            path,
            offset: cut as u64,
        },
        &bytes[cut..],
    );
    assert!(!events.is_empty());
    for ev in &events {
        assert_eq!(ev.session, SESSION);
        assert_eq!(ev.agent.as_deref(), Some(SUBAGENT));
    }
}

#[test]
fn hooks_and_rollouts_merge() {
    let mut r = Replay::new();
    r.hook("SessionStart");
    r.hook("UserPromptSubmit");
    r.hook("PreToolUse");
    let events = main_rollout();
    let repeated: Vec<&AgentEvent> = events
        .iter()
        .filter(|e| {
            matches!(
                (&e.kind, e.dedupe_key.as_deref()),
                (EventKind::PromptSubmitted { .. }, Some("turn-1"))
                    | (EventKind::ToolStarted { .. }, Some("call_01"))
            )
        })
        .collect();
    assert_eq!(repeated.len(), 2);
    for ev in repeated {
        assert!(r.graph.apply(ev).is_empty(), "{ev:?} was applied twice");
    }
}
