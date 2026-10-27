//! Replays recorded Copilot CLI 1.0 hook payloads and an `events.jsonl`.

use std::path::{Path, PathBuf};

use agentscope_adapters::{Adapter, CopilotCli, TranscriptFile};
use agentscope_core::{
    AgentEvent, AgentKind, AgentState, Confidence, EventKind, Graph, HarnessId, RawHookEvent,
};

const SESSION: &str = "7c1e2d3f-4a5b-4c6d-8e9f-0a1b2c3d4e5f";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/copilot-cli/1.0")
}

fn hook(name: &str) -> RawHookEvent {
    let payload = std::fs::read(fixtures().join("hooks").join(format!("{name}.json"))).unwrap();
    RawHookEvent {
        v: 1,
        harness: HarnessId::CopilotCli,
        event: name.split('.').next().unwrap().into(),
        received_at: 1_800_000_000_000,
        pid: Some(11),
        ppid: Some(10),
        payload: serde_json::from_slice(&payload).unwrap(),
    }
}

fn events_file() -> Vec<AgentEvent> {
    let path = fixtures().join(format!("session-state/{SESSION}/events.jsonl"));
    let bytes = std::fs::read(&path).unwrap();
    CopilotCli::with_home(fixtures())
        .parse_transcript_chunk(&TranscriptFile { path, offset: 0 }, &bytes)
}

fn state(graph: &Graph) -> AgentState {
    graph
        .session(&format!("copilot-cli:{SESSION}"))
        .unwrap()
        .state
}

#[test]
fn a_permission_prompt_becomes_an_approval_with_the_requested_tool() {
    let adapter = CopilotCli::with_home(fixtures());
    let mut graph = Graph::new();
    let apply = |graph: &mut Graph, name: &str| -> Vec<EventKind> {
        let events = adapter.normalize_hook(&hook(name));
        for ev in &events {
            graph.apply(ev);
        }
        events.into_iter().map(|e| e.kind).collect()
    };
    apply(&mut graph, "sessionStart");
    assert!(
        apply(&mut graph, "permissionRequest").is_empty(),
        "a permission check alone is not a prompt"
    );
    let kinds = apply(&mut graph, "notification.permission_prompt");
    let [EventKind::ApprovalRequested {
        tool,
        summary,
        detail,
        confidence,
        ..
    }] = kinds.as_slice()
    else {
        panic!("expected one approval, got {kinds:?}");
    };
    assert_eq!(tool, "bash");
    assert_eq!(*confidence, Confidence::Confirmed);
    assert_eq!(summary, "bash: GITHUB_TOKEN=[redacted] gh pr create --fill");
    assert!(!detail.as_ref().unwrap().to_string().contains("ghp_"));
    assert_eq!(state(&graph), AgentState::WaitingApproval);

    // A prompt without a recorded request still shows, from its message.
    let kinds = apply(&mut graph, "notification.permission_prompt");
    assert!(
        matches!(&kinds[0], EventKind::ApprovalRequested { summary, .. } if summary == "Copilot wants to run a shell command")
    );

    assert!(apply(&mut graph, "notification.shell_completed").is_empty());
    apply(&mut graph, "notification.elicitation_dialog");
    apply(&mut graph, "agentStop");
    assert!(graph.snapshot().approvals.is_empty());
    assert_eq!(state(&graph), AgentState::WaitingInput);
    apply(&mut graph, "sessionEnd");
    assert_eq!(state(&graph), AgentState::Ended);
}

#[test]
fn subagent_hooks_name_their_parent_and_type() {
    let adapter = CopilotCli::with_home(fixtures());
    let mut graph = Graph::new();
    for name in [
        "permissionRequest.subagent",
        "notification.permission_prompt",
        "subagentStop",
    ] {
        for ev in adapter.normalize_hook(&hook(name)) {
            graph.apply(&ev);
        }
    }
    let snap = graph.snapshot();
    let sub = snap
        .agents
        .iter()
        .find(|a| a.kind == AgentKind::Subagent)
        .unwrap();
    assert_eq!(sub.id, format!("copilot-cli:{SESSION}/agent-explore-1"));
    assert_eq!(sub.agent_type.as_deref(), Some("explore"));
    assert_eq!(sub.state, AgentState::Ended);
    assert!(
        snap.approvals.is_empty(),
        "the subagent's prompt ended with it"
    );
}

#[test]
fn the_event_log_rebuilds_the_session() {
    let mut graph = Graph::new();
    let events = events_file();
    for ev in &events {
        graph.apply(ev);
    }
    let snap = graph.snapshot();
    let session = &snap.sessions[0];
    assert_eq!(session.cwd, "/work/web");
    assert_eq!(session.model.as_deref(), Some("claude-sonnet-5.5"));
    assert_eq!(session.title.as_deref(), Some("Fix the flaky signup test"));
    assert_eq!(session.state, AgentState::Ended);
    let tokens = session.tokens.unwrap();
    assert_eq!(
        (tokens.input, tokens.output, tokens.cached),
        (3500, 400, Some(9000))
    );

    let sub = snap
        .agents
        .iter()
        .find(|a| a.kind == AgentKind::Subagent)
        .unwrap();
    assert_eq!(sub.id, format!("copilot-cli:{SESSION}/agent-explore-1"));

    let turn_ends = events
        .iter()
        .filter(|e| e.kind == EventKind::TurnCompleted)
        .count();
    assert_eq!(
        turn_ends, 1,
        "a turn that ends in a tool call is not the end"
    );
    assert!(events.iter().any(|e| matches!(
        &e.kind,
        EventKind::ToolFailed { error: Some(err) } if err == "1 failing"
    )));
}
