//! Replays recorded Cursor hook payloads from `fixtures/`.

use std::path::{Path, PathBuf};

use agentscope_adapters::{Adapter, Cursor};
use agentscope_core::store::Duration;
use agentscope_core::{
    AgentKind, AgentState, Confidence, EventKind, Graph, HarnessId, RawHookEvent,
};

const SESSION: &str = "c0ffee00-1111-4222-8333-444455556666";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/cursor/hooks-v1")
}

fn hook(name: &str, at: u64) -> RawHookEvent {
    let payload = std::fs::read(fixtures().join("hooks").join(format!("{name}.json"))).unwrap();
    RawHookEvent {
        v: 1,
        harness: HarnessId::Cursor,
        event: name.split('.').next().unwrap().into(),
        received_at: 1_800_000_000_000 + at,
        pid: Some(11),
        ppid: Some(10),
        payload: serde_json::from_slice(&payload).unwrap(),
    }
}

struct Replay {
    adapter: Cursor,
    graph: Graph,
    t: u64,
}

impl Replay {
    fn new() -> Self {
        Self {
            adapter: Cursor::with_home(fixtures()),
            graph: Graph::new(),
            t: 0,
        }
    }

    fn hook(&mut self, name: &str) -> Vec<EventKind> {
        self.t += 1000;
        let events = self.adapter.normalize_hook(&hook(name, self.t));
        for ev in &events {
            self.graph.apply(ev);
        }
        events.into_iter().map(|e| e.kind).collect()
    }

    fn now(&self) -> agentscope_core::Timestamp {
        chrono::DateTime::from_timestamp_millis(1_800_000_000_000 + self.t as i64).unwrap()
    }

    fn state(&self) -> AgentState {
        self.graph
            .session(&format!("cursor:{SESSION}"))
            .unwrap()
            .state
    }
}

#[test]
fn a_shell_call_without_a_result_becomes_a_maybe_waiting_approval() {
    let mut r = Replay::new();
    r.hook("sessionStart");
    assert_eq!(
        r.graph.session(&format!("cursor:{SESSION}")).unwrap().cwd,
        "/work/app"
    );
    r.hook("beforeSubmitPrompt");
    let kinds = r.hook("preToolUse");
    assert!(matches!(kinds[1], EventKind::ApprovalPossible { .. }));
    assert_eq!(r.state(), AgentState::RunningTool);

    let now = r.now();
    let due = r
        .graph
        .due_possible_approvals(now + Duration::seconds(10), Duration::seconds(6));
    assert_eq!(due.len(), 1);
    for ev in &due {
        r.graph.apply(ev);
    }
    assert_eq!(r.state(), AgentState::MaybeWaiting);
    let approval = &r.graph.snapshot().approvals[0];
    assert_eq!(approval.confidence, Confidence::Inferred);
    assert_eq!(approval.summary, "Shell: npm install react@19 react-dom@19");

    r.hook("postToolUse");
    assert!(r.graph.snapshot().approvals.is_empty());
    assert_eq!(r.state(), AgentState::Thinking);
}

#[test]
fn reads_and_quick_results_never_look_like_prompts() {
    let mut r = Replay::new();
    r.hook("sessionStart");
    let kinds = r.hook("preToolUse.read");
    assert_eq!(kinds.len(), 1, "reads do not prompt");
    r.hook("preToolUse");
    r.hook("postToolUse");
    let now = r.now();
    assert!(r
        .graph
        .due_possible_approvals(now + Duration::seconds(60), Duration::seconds(6))
        .is_empty());
}

#[test]
fn subagents_map_back_onto_their_session() {
    let mut r = Replay::new();
    r.hook("sessionStart");
    r.hook("subagentStart");
    r.hook("preToolUse.subagent");
    let snap = r.graph.snapshot();
    assert_eq!(
        snap.sessions.len(),
        1,
        "the subagent is not its own session"
    );
    let sub = snap
        .agents
        .iter()
        .find(|a| a.kind == AgentKind::Subagent)
        .unwrap();
    assert_eq!(sub.id, format!("cursor:{SESSION}/sub-7f3a"));
    assert_eq!(sub.agent_type.as_deref(), Some("explore"));
    assert_eq!(sub.state, AgentState::RunningTool);

    r.hook("subagentStop");
    let sub = r
        .graph
        .agent(&format!("cursor:{SESSION}/sub-7f3a"))
        .unwrap();
    assert_eq!(sub.state, AgentState::Ended);

    r.hook("stop");
    assert_eq!(r.state(), AgentState::WaitingInput);
    r.hook("sessionEnd");
    assert_eq!(r.state(), AgentState::Ended);
}

#[test]
fn a_rejected_command_resolves_as_denied() {
    let mut r = Replay::new();
    r.hook("sessionStart");
    let kinds = r.hook("postToolUseFailure");
    assert!(matches!(
        kinds[0],
        EventKind::ApprovalResolved {
            resolution: agentscope_core::Resolution::Denied,
            ..
        }
    ));
}
