//! Replays recorded Claude Code 2.1 payloads and transcripts from `fixtures/`.

use std::path::{Path, PathBuf};

use agentscope_adapters::{Adapter, ClaudeCode, TranscriptFile};
use agentscope_core::{
    AgentKind, AgentState, Confidence, EventKind, Graph, HarnessId, RawHookEvent, Resolution,
};

const SESSION: &str = "5f0c7a52-1d1e-4c3b-9d7e-0b8e6f1a2c11";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/claude-code/2.1")
}

fn hook(name: &str, at: u64) -> RawHookEvent {
    let payload = std::fs::read(fixtures().join("hooks").join(format!("{name}.json"))).unwrap();
    RawHookEvent {
        v: 1,
        harness: HarnessId::ClaudeCode,
        event: name.split('.').next().unwrap().into(),
        received_at: 1_800_000_000_000 + at,
        pid: Some(11),
        ppid: Some(10),
        payload: serde_json::from_slice(&payload).unwrap(),
    }
}

fn adapter() -> ClaudeCode {
    ClaudeCode::with_config_dir(fixtures())
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

    fn session_state(&self) -> AgentState {
        self.graph
            .session(&format!("claude-code:{SESSION}"))
            .unwrap()
            .state
    }
}

#[test]
fn hook_session_walkthrough() {
    let mut r = Replay::new();
    r.hook("SessionStart");
    let session = r.graph.session(&format!("claude-code:{SESSION}")).unwrap();
    assert_eq!(session.cwd, "/work/shop");
    assert_eq!(session.model.as_deref(), Some("claude-sonnet-5-5"));

    r.hook("UserPromptSubmit");
    assert_eq!(r.session_state(), AgentState::Thinking);

    r.hook("PreToolUse");
    assert_eq!(r.session_state(), AgentState::RunningTool);

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
    assert_eq!(summary, "Bash: STRIPE_SECRET_KEY=[redacted] npm run e2e");
    assert!(!detail.as_ref().unwrap().to_string().contains("sk_test_123"));
    assert_eq!(r.session_state(), AgentState::WaitingApproval);
    assert_eq!(r.graph.snapshot().approvals.len(), 1);

    assert!(
        r.hook("Notification.permission_prompt").is_empty(),
        "the notification duplicates PermissionRequest"
    );

    r.hook("PostToolUse");
    assert!(r.graph.snapshot().approvals.is_empty());
    assert_eq!(r.session_state(), AgentState::Thinking);

    r.hook("SubagentStart");
    r.hook("PreToolUse.subagent");
    let snap = r.graph.snapshot();
    let sub = snap
        .agents
        .iter()
        .find(|a| a.kind == AgentKind::Subagent)
        .unwrap();
    assert_eq!(sub.agent_type.as_deref(), Some("Explore"));
    assert_eq!(sub.state, AgentState::RunningTool);
    assert_eq!(
        sub.current_activity.as_ref().unwrap().summary,
        "Grep: applyCoupon"
    );

    r.hook("SubagentStop");
    r.hook("Stop");
    assert_eq!(r.session_state(), AgentState::WaitingInput);

    r.hook("SessionEnd");
    assert_eq!(r.session_state(), AgentState::Ended);
}

#[test]
fn denied_and_failed_tools_resolve_approvals() {
    let mut r = Replay::new();
    r.hook("PermissionRequest");
    r.hook("PermissionDenied");
    let snap = r.graph.snapshot();
    assert!(snap.approvals.is_empty());
    assert_eq!(r.session_state(), AgentState::Thinking);

    let kinds = r.hook("PostToolUseFailure");
    assert_eq!(
        kinds,
        [EventKind::ToolFailed {
            error: Some("Exit code 1".into())
        }]
    );
    let _ = Resolution::Denied;
}

#[test]
fn every_fixture_payload_is_understood() {
    for entry in std::fs::read_dir(fixtures().join("hooks")).unwrap() {
        let name = entry.unwrap().path();
        let name = name.file_stem().unwrap().to_str().unwrap();
        let events = adapter().normalize_hook(&hook(name, 0));
        if name != "Notification.permission_prompt" {
            assert!(!events.is_empty(), "{name} produced no events");
        }
        for ev in events {
            assert_eq!(ev.session, SESSION, "{name}");
        }
    }
}

#[test]
fn foreign_payloads_are_ignored() {
    let mut raw = hook("Stop", 0);
    raw.payload = serde_json::json!({ "sessionId": "copilot", "hookEventName": "Stop" });
    assert!(adapter().normalize_hook(&raw).is_empty());
}

fn transcript(path: &Path) -> Vec<agentscope_core::AgentEvent> {
    let bytes = std::fs::read(path).unwrap();
    adapter().parse_transcript_chunk(
        &TranscriptFile {
            path: path.to_owned(),
            offset: 0,
        },
        &bytes,
    )
}

#[test]
fn transcripts_rebuild_the_session_and_its_subagents() {
    let project = fixtures().join("projects/-work-shop");
    let main = project.join(format!("{SESSION}.jsonl"));
    let sub = project.join(format!("{SESSION}/subagents/agent-a7c1e9.jsonl"));

    let mut graph = Graph::new();
    for ev in transcript(&main).iter().chain(transcript(&sub).iter()) {
        graph.apply(ev);
    }
    let snap = graph.snapshot();
    let session = &snap.sessions[0];
    assert_eq!(session.cwd, "/work/shop");
    assert_eq!(session.title.as_deref(), Some("Fix failing checkout tests"));
    assert_eq!(session.model.as_deref(), Some("claude-sonnet-5-5"));
    assert_eq!(
        session.transcript_path.as_deref(),
        Some(main.to_str().unwrap())
    );
    assert_eq!(
        session.state,
        AgentState::WaitingInput,
        "the last turn ended"
    );

    let tokens = session.tokens.unwrap();
    // msg_01 is split over two lines and counted once.
    assert_eq!(tokens.input, 1010 + 5 + 5 + 100 + 120);
    assert_eq!(tokens.output, 200 + 50 + 30 + 20 + 10);
    assert_eq!(tokens.cached, Some(5000 + 6000 + 7000));

    let sub = snap
        .agents
        .iter()
        .find(|a| a.kind == AgentKind::Subagent)
        .unwrap();
    assert_eq!(sub.id, format!("claude-code:{SESSION}/a7c1e9"));
    assert_eq!(sub.agent_type.as_deref(), Some("Explore"));
    assert_eq!(sub.description.as_deref(), Some("Find coupon logic"));
    assert_eq!(sub.state, AgentState::Ended);
}

#[test]
fn transcripts_skip_injected_prompts() {
    let main = fixtures().join(format!("projects/-work-shop/{SESSION}.jsonl"));
    let prompts: Vec<String> = transcript(&main)
        .into_iter()
        .filter_map(|e| match e.kind {
            EventKind::PromptSubmitted { prompt } => Some(prompt),
            _ => None,
        })
        .collect();
    assert_eq!(prompts, ["Fix the failing checkout tests"]);
}

#[test]
fn transcript_events_merge_with_hooks() {
    // The hook saw the tool call first; the transcript copy must not rewind state.
    let mut r = Replay::new();
    r.hook("SessionStart");
    r.hook("UserPromptSubmit");
    r.hook("PreToolUse");
    let main = fixtures().join(format!("projects/-work-shop/{SESSION}.jsonl"));
    let events = transcript(&main);
    let tool_started = events
        .iter()
        .find(|e| {
            e.dedupe_key.as_deref() == Some("toolu_01A")
                && matches!(e.kind, EventKind::ToolStarted { .. })
        })
        .unwrap();
    assert!(r.graph.apply(tool_started).is_empty());
    let started = events
        .iter()
        .find(|e| matches!(e.kind, EventKind::SessionStarted { .. }))
        .unwrap();
    r.graph.apply(started);
    assert_eq!(r.session_state(), AgentState::RunningTool);
}
