//! Applies [`AgentEvent`]s to the in-memory graph of sessions, agents, and
//! approvals. Pure and synchronous: the caller owns IO and persistence.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::event::{AgentEvent, EventKind, EventSource};
use crate::model::*;

/// How many dedupe keys to remember. Old keys only matter while a transcript
/// lags behind its hooks, so a bounded window is enough.
const DEDUPE_WINDOW: usize = 50_000;

const TITLE_MAX_CHARS: usize = 80;

/// Ids of the entities an event touched, so the store can persist only those.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Changes {
    pub sessions: BTreeSet<String>,
    pub agents: BTreeSet<String>,
    pub approvals: BTreeSet<String>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty() && self.agents.is_empty() && self.approvals.is_empty()
    }

    pub fn extend(&mut self, other: Changes) {
        self.sessions.extend(other.sessions);
        self.agents.extend(other.agents);
        self.approvals.extend(other.approvals);
    }
}

/// What the UI renders.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// Most recently active first.
    pub sessions: Vec<Session>,
    pub agents: Vec<Agent>,
    /// Unresolved approvals, oldest first.
    pub approvals: Vec<PendingApproval>,
    /// Live sessions that need the user. Drives the tray badge.
    pub waiting_count: u32,
}

#[derive(Debug, Default)]
pub struct Graph {
    sessions: BTreeMap<String, Session>,
    agents: BTreeMap<String, Agent>,
    approvals: BTreeMap<String, PendingApproval>,
    seen: HashMap<String, EventSource>,
    seen_order: VecDeque<String>,
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Rebuilds a graph from persisted entities.
    pub fn from_parts(
        sessions: impl IntoIterator<Item = Session>,
        agents: impl IntoIterator<Item = Agent>,
        approvals: impl IntoIterator<Item = PendingApproval>,
    ) -> Self {
        Self {
            sessions: sessions.into_iter().map(|s| (s.id.clone(), s)).collect(),
            agents: agents.into_iter().map(|a| (a.id.clone(), a)).collect(),
            approvals: approvals.into_iter().map(|a| (a.id.clone(), a)).collect(),
            ..Self::default()
        }
    }

    pub fn session(&self, id: &str) -> Option<&Session> {
        self.sessions.get(id)
    }

    pub fn agent(&self, id: &str) -> Option<&Agent> {
        self.agents.get(id)
    }

    pub fn approval(&self, id: &str) -> Option<&PendingApproval> {
        self.approvals.get(id)
    }

    pub fn sessions(&self) -> impl Iterator<Item = &Session> {
        self.sessions.values()
    }

    /// Sets the repo a session's cwd belongs to. Returns whether it changed.
    pub fn set_repo(&mut self, session_id: &str, repo: Option<Repo>) -> bool {
        match self.sessions.get_mut(session_id) {
            Some(session) if session.repo != repo => {
                session.repo = repo;
                true
            }
            _ => false,
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let mut sessions: Vec<Session> = self.sessions.values().cloned().collect();
        sessions.sort_by_key(|s| std::cmp::Reverse(s.last_event_at));
        let mut approvals: Vec<PendingApproval> = self
            .approvals
            .values()
            .filter(|a| a.resolved_at.is_none())
            .cloned()
            .collect();
        approvals.sort_by_key(|a| a.requested_at);
        let waiting_count = sessions
            .iter()
            .filter(|s| s.ended_at.is_none() && s.state.needs_user())
            .count() as u32;
        Snapshot {
            sessions,
            agents: self.agents.values().cloned().collect(),
            approvals,
            waiting_count,
        }
    }

    pub fn apply(&mut self, ev: &AgentEvent) -> Changes {
        let mut changes = Changes::default();
        if self.is_duplicate(ev) {
            return changes;
        }
        let session_id = Session::key(ev.harness, &ev.session);
        if !self.sessions.contains_key(&session_id) {
            // Unknown events never create sessions; everything else does,
            // since transcripts and late installs can miss SessionStarted.
            if matches!(ev.kind, EventKind::Unknown { .. }) {
                return changes;
            }
            self.insert_session(ev, &session_id);
        }
        let reopen = self.sessions[&session_id]
            .ended_at
            .is_some_and(|ended| ev.at > ended && ev.kind.is_activity());
        if reopen {
            self.sessions
                .get_mut(&session_id)
                .expect("checked above")
                .ended_at = None;
        }
        let agent_id = self.ensure_agent(ev, &session_id, &mut changes);

        match &ev.kind {
            EventKind::SessionStarted {
                cwd,
                model,
                transcript_path,
                pid,
            } => {
                let session = self.sessions.get_mut(&session_id).expect("inserted above");
                if !cwd.is_empty() {
                    session.cwd.clone_from(cwd);
                }
                if model.is_some() {
                    session.model.clone_from(model);
                }
                if transcript_path.is_some() {
                    session.transcript_path.clone_from(transcript_path);
                }
                if pid.is_some() {
                    session.pid = *pid;
                }
                session.ended_at = None;
                // A transcript replayed after hooks must not rewind the state.
                if ev.source == EventSource::Hook
                    || self.agents[&agent_id].state == AgentState::Unknown
                {
                    self.set_agent_state(&agent_id, AgentState::Starting, ev.at);
                }
            }
            EventKind::SessionEnded => {
                self.sessions
                    .get_mut(&session_id)
                    .expect("inserted above")
                    .ended_at = Some(ev.at);
                for agent in self
                    .agents
                    .values_mut()
                    .filter(|a| a.session_id == session_id)
                {
                    if agent.state.is_live() {
                        agent.state = AgentState::Ended;
                        agent.ended_at.get_or_insert(ev.at);
                        agent.current_activity = None;
                        changes.agents.insert(agent.id.clone());
                    }
                }
                self.resolve_approvals(
                    |a| a.session_id == session_id,
                    Resolution::Cancelled,
                    ev.at,
                    &mut changes,
                );
            }
            EventKind::SessionInfo { cwd, model, title } => {
                let session = self.sessions.get_mut(&session_id).expect("inserted above");
                if let Some(cwd) = cwd.as_ref().filter(|c| !c.is_empty()) {
                    session.cwd.clone_from(cwd);
                }
                if model.is_some() {
                    session.model.clone_from(model);
                }
                if let Some(title) = title.as_deref().and_then(title_from_prompt) {
                    session.title = Some(title);
                }
            }
            EventKind::PromptSubmitted { prompt } => {
                let session = self.sessions.get_mut(&session_id).expect("inserted above");
                if session.title.is_none() {
                    session.title = title_from_prompt(prompt);
                }
                // The user is typing again, so any prompt they were shown is gone.
                self.resolve_approvals(
                    |a| a.session_id == session_id,
                    Resolution::Unknown,
                    ev.at,
                    &mut changes,
                );
                self.set_agent_state(&agent_id, AgentState::Thinking, ev.at);
            }
            EventKind::AgentStarted { .. } => {
                // `ensure_agent` created it with the right parent and type.
                if self.agents[&agent_id].state == AgentState::Unknown {
                    self.set_agent_state(&agent_id, AgentState::Starting, ev.at);
                }
            }
            EventKind::AgentStopped { result } => {
                let agent = self.agents.get_mut(&agent_id).expect("ensured above");
                agent.state = AgentState::Ended;
                agent.ended_at = Some(ev.at);
                agent.result = Some(*result);
                agent.current_activity = None;
                self.resolve_approvals(
                    |a| a.agent_id == agent_id,
                    Resolution::Unknown,
                    ev.at,
                    &mut changes,
                );
            }
            EventKind::ToolStarted { tool, summary } => {
                let agent = self.agents.get_mut(&agent_id).expect("ensured above");
                agent.state = AgentState::RunningTool;
                agent.current_activity = Some(Activity {
                    kind: ActivityKind::Tool,
                    tool: Some(tool.clone()),
                    summary: summary.clone(),
                    started_at: ev.at,
                });
            }
            EventKind::ToolFinished | EventKind::ToolFailed { .. } => {
                let resolution = match ev.kind {
                    EventKind::ToolFinished => Resolution::Approved,
                    _ => Resolution::Unknown,
                };
                self.resolve_approvals(|a| a.agent_id == agent_id, resolution, ev.at, &mut changes);
                let agent = self.agents.get_mut(&agent_id).expect("ensured above");
                agent.state = AgentState::Thinking;
                agent.current_activity = None;
            }
            EventKind::ApprovalRequested {
                approval_id,
                tool,
                summary,
                detail,
                confidence,
            } => {
                let id = format!("{session_id}#{approval_id}");
                self.approvals.insert(
                    id.clone(),
                    PendingApproval {
                        id: id.clone(),
                        session_id: session_id.clone(),
                        agent_id: agent_id.clone(),
                        harness: ev.harness,
                        tool: tool.clone(),
                        summary: summary.clone(),
                        detail: detail.clone(),
                        requested_at: ev.at,
                        confidence: *confidence,
                        resolved_at: None,
                        resolution: None,
                    },
                );
                changes.approvals.insert(id);
                let state = match confidence {
                    Confidence::Confirmed => AgentState::WaitingApproval,
                    Confidence::Inferred => AgentState::MaybeWaiting,
                };
                self.set_agent_state(&agent_id, state, ev.at);
            }
            EventKind::ApprovalResolved {
                approval_id,
                resolution,
            } => {
                let target = approval_id.as_ref().map(|id| format!("{session_id}#{id}"));
                self.resolve_approvals(
                    |a| match &target {
                        Some(id) => &a.id == id,
                        None => a.agent_id == agent_id,
                    },
                    *resolution,
                    ev.at,
                    &mut changes,
                );
                let agent = self.agents.get_mut(&agent_id).expect("ensured above");
                if matches!(
                    agent.state,
                    AgentState::WaitingApproval | AgentState::MaybeWaiting
                ) {
                    agent.state = AgentState::Thinking;
                }
            }
            EventKind::WaitingForInput | EventKind::TurnCompleted => {
                self.resolve_approvals(
                    |a| a.agent_id == agent_id,
                    Resolution::Unknown,
                    ev.at,
                    &mut changes,
                );
                let agent = self.agents.get_mut(&agent_id).expect("ensured above");
                agent.state = AgentState::WaitingInput;
                agent.current_activity = None;
            }
            EventKind::Compacted => {
                let agent = self.agents.get_mut(&agent_id).expect("ensured above");
                agent.current_activity = Some(Activity {
                    kind: ActivityKind::Compact,
                    tool: None,
                    summary: "Compacting context".into(),
                    started_at: ev.at,
                });
            }
            EventKind::TokensReported { tokens } => {
                self.sessions
                    .get_mut(&session_id)
                    .expect("inserted above")
                    .tokens = Some(*tokens);
            }
            EventKind::TokensUsed { tokens } => {
                let session = self.sessions.get_mut(&session_id).expect("inserted above");
                let total = session.tokens.get_or_insert_with(Tokens::default);
                total.input += tokens.input;
                total.output += tokens.output;
                if let Some(cached) = tokens.cached {
                    total.cached = Some(total.cached.unwrap_or(0) + cached);
                }
            }
            EventKind::Heartbeat { pid } => {
                self.sessions
                    .get_mut(&session_id)
                    .expect("inserted above")
                    .pid = Some(*pid);
            }
            EventKind::Unknown { .. } => {}
        }

        // Sources arrive out of order (a scan can beat a transcript backfill),
        // so the earliest event seen marks the start.
        if let Some(agent) = self.agents.get_mut(&agent_id) {
            if ev.at < agent.started_at {
                agent.started_at = ev.at;
            }
        }
        changes.agents.insert(agent_id);
        let session = self.sessions.get_mut(&session_id).expect("inserted above");
        if ev.at > session.last_event_at {
            session.last_event_at = ev.at;
        }
        if ev.at < session.started_at {
            session.started_at = ev.at;
        }
        self.refresh_session_state(&session_id);
        changes.sessions.insert(session_id);
        changes
    }

    /// Hooks win: a transcript or scan event with a key already seen is dropped,
    /// and a hook event is dropped only if a hook already delivered it.
    fn is_duplicate(&mut self, ev: &AgentEvent) -> bool {
        let Some(key) = &ev.dedupe_key else {
            return false;
        };
        let key = format!("{}:{}:{}:{}", ev.harness, ev.session, ev.kind.name(), key);
        match self.seen.get(&key) {
            Some(EventSource::Hook) => true,
            Some(_) if ev.source != EventSource::Hook => true,
            Some(_) => {
                self.seen.insert(key, ev.source);
                false
            }
            None => {
                self.seen.insert(key.clone(), ev.source);
                self.seen_order.push_back(key);
                if self.seen_order.len() > DEDUPE_WINDOW {
                    if let Some(old) = self.seen_order.pop_front() {
                        self.seen.remove(&old);
                    }
                }
                false
            }
        }
    }

    fn insert_session(&mut self, ev: &AgentEvent, session_id: &str) {
        let root_agent_id = Agent::key(session_id, Agent::MAIN);
        self.sessions.insert(
            session_id.to_owned(),
            Session {
                id: session_id.to_owned(),
                harness: ev.harness,
                native_id: ev.session.clone(),
                title: None,
                cwd: String::new(),
                repo: None,
                model: None,
                started_at: ev.at,
                last_event_at: ev.at,
                ended_at: None,
                state: AgentState::Unknown,
                pid: None,
                terminal: None,
                transcript_path: None,
                tokens: None,
                root_agent_id: root_agent_id.clone(),
            },
        );
        self.agents.insert(
            root_agent_id.clone(),
            new_agent(root_agent_id, session_id, None, ev),
        );
    }

    /// Returns the id of the agent `ev` is about, creating it if needed.
    fn ensure_agent(&mut self, ev: &AgentEvent, session_id: &str, changes: &mut Changes) -> String {
        let native = ev.agent.as_deref().unwrap_or(Agent::MAIN);
        let id = Agent::key(session_id, native);
        if !self.agents.contains_key(&id) {
            let parent = match &ev.kind {
                EventKind::AgentStarted { parent, .. } => parent.as_deref(),
                _ => None,
            };
            let parent_id = Agent::key(session_id, parent.unwrap_or(Agent::MAIN));
            self.agents.insert(
                id.clone(),
                new_agent(id.clone(), session_id, Some(parent_id), ev),
            );
            changes.agents.insert(id.clone());
        }
        if let EventKind::AgentStarted {
            agent_type,
            description,
            ..
        } = &ev.kind
        {
            let agent = self.agents.get_mut(&id).expect("inserted above");
            if agent_type.is_some() {
                agent.agent_type.clone_from(agent_type);
            }
            if description.is_some() {
                agent.description.clone_from(description);
            }
        }
        id
    }

    fn set_agent_state(&mut self, agent_id: &str, state: AgentState, at: Timestamp) {
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.state = state;
            if state.is_live() {
                agent.ended_at = None;
            } else {
                agent.ended_at.get_or_insert(at);
            }
        }
    }

    fn resolve_approvals(
        &mut self,
        mut matches: impl FnMut(&PendingApproval) -> bool,
        resolution: Resolution,
        at: Timestamp,
        changes: &mut Changes,
    ) {
        for approval in self.approvals.values_mut() {
            if approval.resolved_at.is_none() && matches(approval) {
                approval.resolved_at = Some(at);
                approval.resolution = Some(resolution);
                changes.approvals.insert(approval.id.clone());
            }
        }
    }

    /// A session shows its most urgent agent: a pending approval anywhere in
    /// the tree beats whatever the main agent is doing.
    fn refresh_session_state(&mut self, session_id: &str) {
        let Some(session) = self.sessions.get(session_id) else {
            return;
        };
        let agents: Vec<&Agent> = self
            .agents
            .values()
            .filter(|a| a.session_id == session_id)
            .collect();
        let main_state = self
            .agents
            .get(&session.root_agent_id)
            .map(|a| a.state)
            .unwrap_or_default();
        let state = if session.ended_at.is_some() {
            AgentState::Ended
        } else if agents
            .iter()
            .any(|a| a.state == AgentState::WaitingApproval)
        {
            AgentState::WaitingApproval
        } else if agents.iter().any(|a| a.state == AgentState::MaybeWaiting) {
            AgentState::MaybeWaiting
        } else {
            main_state
        };
        self.sessions
            .get_mut(session_id)
            .expect("checked above")
            .state = state;
    }
}

fn new_agent(id: String, session_id: &str, parent: Option<String>, ev: &AgentEvent) -> Agent {
    Agent {
        id,
        session_id: session_id.to_owned(),
        kind: if parent.is_some() {
            AgentKind::Subagent
        } else {
            AgentKind::Main
        },
        parent_agent_id: parent,
        agent_type: None,
        description: None,
        state: AgentState::Unknown,
        current_activity: None,
        started_at: ev.at,
        ended_at: None,
        result: None,
    }
}

fn title_from_prompt(prompt: &str) -> Option<String> {
    let line = prompt.lines().map(str::trim).find(|l| !l.is_empty())?;
    let mut title: String = line.chars().take(TITLE_MAX_CHARS).collect();
    if line.chars().count() > TITLE_MAX_CHARS {
        title.push('…');
    }
    Some(title)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HarnessId;
    use chrono::{Duration, TimeZone, Utc};

    struct Script {
        t: i64,
        graph: Graph,
    }

    impl Script {
        fn new() -> Self {
            Self {
                t: 0,
                graph: Graph::new(),
            }
        }

        fn ev(&mut self, agent: Option<&str>, key: Option<&str>, kind: EventKind) -> AgentEvent {
            self.t += 1;
            AgentEvent {
                source: EventSource::Hook,
                at: Utc.timestamp_opt(1_800_000_000, 0).unwrap() + Duration::seconds(self.t),
                harness: HarnessId::ClaudeCode,
                session: "s1".into(),
                agent: agent.map(Into::into),
                dedupe_key: key.map(Into::into),
                kind,
            }
        }

        fn apply(&mut self, agent: Option<&str>, kind: EventKind) -> Changes {
            let ev = self.ev(agent, None, kind);
            self.graph.apply(&ev)
        }

        fn session(&self) -> &Session {
            self.graph.session("claude-code:s1").unwrap()
        }

        fn agent(&self, native: &str) -> &Agent {
            self.graph
                .agent(&format!("claude-code:s1/{native}"))
                .unwrap()
        }
    }

    fn started() -> EventKind {
        EventKind::SessionStarted {
            cwd: "/repo".into(),
            model: Some("model-x".into()),
            transcript_path: None,
            pid: Some(42),
        }
    }

    fn approval(id: &str) -> EventKind {
        EventKind::ApprovalRequested {
            approval_id: id.into(),
            tool: "Bash".into(),
            summary: "Bash: rm -rf build".into(),
            detail: None,
            confidence: Confidence::Confirmed,
        }
    }

    #[test]
    fn session_lifecycle() {
        let mut s = Script::new();
        s.apply(None, started());
        assert_eq!(s.session().cwd, "/repo");
        assert_eq!(s.session().state, AgentState::Starting);

        s.apply(
            None,
            EventKind::PromptSubmitted {
                prompt: "\n  Fix the tests\nplease".into(),
            },
        );
        assert_eq!(s.session().title.as_deref(), Some("Fix the tests"));
        assert_eq!(s.session().state, AgentState::Thinking);

        s.apply(
            None,
            EventKind::ToolStarted {
                tool: "Bash".into(),
                summary: "Bash: npm test".into(),
            },
        );
        assert_eq!(s.session().state, AgentState::RunningTool);
        assert_eq!(
            s.agent("main").current_activity.as_ref().unwrap().summary,
            "Bash: npm test"
        );

        s.apply(None, EventKind::ToolFinished);
        s.apply(None, EventKind::TurnCompleted);
        assert_eq!(s.session().state, AgentState::WaitingInput);
        assert_eq!(s.graph.snapshot().waiting_count, 1);

        s.apply(None, EventKind::SessionEnded);
        assert_eq!(s.session().state, AgentState::Ended);
        assert!(s.session().ended_at.is_some());
        assert_eq!(s.agent("main").state, AgentState::Ended);
        assert_eq!(s.graph.snapshot().waiting_count, 0);
    }

    #[test]
    fn subagent_approval_bubbles_up_to_session() {
        let mut s = Script::new();
        s.apply(None, started());
        s.apply(
            Some("a1"),
            EventKind::AgentStarted {
                parent: None,
                agent_type: Some("Explore".into()),
                description: Some("Find the config loader".into()),
            },
        );
        s.apply(
            Some("a2"),
            EventKind::AgentStarted {
                parent: Some("a1".into()),
                agent_type: None,
                description: None,
            },
        );
        assert_eq!(s.agent("a1").kind, AgentKind::Subagent);
        assert_eq!(
            s.agent("a1").parent_agent_id.as_deref(),
            Some("claude-code:s1/main")
        );
        assert_eq!(
            s.agent("a2").parent_agent_id.as_deref(),
            Some("claude-code:s1/a1")
        );

        s.apply(Some("a2"), approval("t1"));
        assert_eq!(s.session().state, AgentState::WaitingApproval);
        let snap = s.graph.snapshot();
        assert_eq!(snap.approvals.len(), 1);
        assert_eq!(snap.approvals[0].agent_id, "claude-code:s1/a2");
        assert_eq!(snap.waiting_count, 1);

        s.apply(Some("a2"), EventKind::ToolFinished);
        let resolved = s.graph.approval("claude-code:s1#t1").unwrap();
        assert_eq!(resolved.resolution, Some(Resolution::Approved));
        assert!(s.graph.snapshot().approvals.is_empty());
        assert_eq!(s.session().state, AgentState::Starting);

        s.apply(
            Some("a2"),
            EventKind::AgentStopped {
                result: AgentResult::Completed,
            },
        );
        assert_eq!(s.agent("a2").state, AgentState::Ended);
        assert_eq!(s.agent("a2").result, Some(AgentResult::Completed));
    }

    #[test]
    fn session_end_cancels_open_approvals() {
        let mut s = Script::new();
        s.apply(None, approval("t1"));
        s.apply(None, EventKind::SessionEnded);
        let a = s.graph.approval("claude-code:s1#t1").unwrap();
        assert_eq!(a.resolution, Some(Resolution::Cancelled));
    }

    #[test]
    fn hooks_win_over_transcripts() {
        let mut s = Script::new();
        let tool = || EventKind::ToolStarted {
            tool: "Read".into(),
            summary: "Read a.rs".into(),
        };

        let mut from_transcript = s.ev(None, Some("tu_1"), tool());
        from_transcript.source = EventSource::Transcript;
        let from_hook = s.ev(None, Some("tu_1"), tool());

        assert!(!s.graph.apply(&from_hook).is_empty());
        assert!(
            s.graph.apply(&from_transcript).is_empty(),
            "transcript copy is dropped"
        );
        assert!(
            s.graph.apply(&from_hook).is_empty(),
            "hook copy is dropped once"
        );

        let mut s = Script::new();
        let mut first = s.ev(None, Some("tu_2"), tool());
        first.source = EventSource::Transcript;
        let later_hook = s.ev(None, Some("tu_2"), tool());
        assert!(!s.graph.apply(&first).is_empty());
        assert!(
            !s.graph.apply(&later_hook).is_empty(),
            "hook still applies after a transcript"
        );
    }

    #[test]
    fn activity_after_the_end_reopens_a_session() {
        let mut s = Script::new();
        s.apply(None, started());
        s.apply(None, EventKind::SessionEnded);
        // A stale transcript line from before the end changes nothing.
        let mut old = s.ev(None, None, EventKind::Compacted);
        old.at -= Duration::seconds(60);
        s.graph.apply(&old);
        assert_eq!(s.session().state, AgentState::Ended);

        s.apply(
            None,
            EventKind::PromptSubmitted {
                prompt: "again".into(),
            },
        );
        assert!(s.session().ended_at.is_none());
        assert_eq!(s.session().state, AgentState::Thinking);
    }

    #[test]
    fn a_new_prompt_clears_unanswered_approvals() {
        let mut s = Script::new();
        s.apply(None, approval("t1"));
        s.apply(
            None,
            EventKind::PromptSubmitted {
                prompt: "never mind".into(),
            },
        );
        assert!(s.graph.snapshot().approvals.is_empty());
        let a = s.graph.approval("claude-code:s1#t1").unwrap();
        assert_eq!(a.resolution, Some(Resolution::Unknown));
    }

    #[test]
    fn session_info_and_token_usage() {
        let mut s = Script::new();
        s.apply(
            None,
            EventKind::PromptSubmitted {
                prompt: "first prompt".into(),
            },
        );
        s.apply(
            None,
            EventKind::SessionInfo {
                cwd: Some("/repo".into()),
                model: Some("m".into()),
                title: Some("Harness title".into()),
            },
        );
        s.apply(
            None,
            EventKind::SessionInfo {
                cwd: None,
                model: None,
                title: None,
            },
        );
        assert_eq!(s.session().title.as_deref(), Some("Harness title"));
        assert_eq!(s.session().cwd, "/repo");
        assert_eq!(s.session().model.as_deref(), Some("m"));

        let used = |input, cached| EventKind::TokensUsed {
            tokens: Tokens {
                input,
                output: 1,
                cached,
            },
        };
        s.apply(None, used(10, None));
        s.apply(None, used(5, Some(7)));
        assert_eq!(
            s.session().tokens,
            Some(Tokens {
                input: 15,
                output: 2,
                cached: Some(7)
            })
        );
    }

    #[test]
    fn replayed_starts_do_not_rewind_state() {
        let mut s = Script::new();
        s.apply(None, started());
        s.apply(
            None,
            EventKind::ToolStarted {
                tool: "Bash".into(),
                summary: "Bash: ls".into(),
            },
        );
        let mut replay = s.ev(None, None, started());
        replay.source = EventSource::Transcript;
        s.graph.apply(&replay);
        assert_eq!(s.session().state, AgentState::RunningTool);
    }

    #[test]
    fn unknown_events_do_not_create_sessions() {
        let mut s = Script::new();
        let changes = s.apply(
            None,
            EventKind::Unknown {
                event: "Weird".into(),
                raw: serde_json::json!({}),
            },
        );
        assert!(changes.is_empty());
        assert!(s.graph.snapshot().sessions.is_empty());
    }

    #[test]
    fn long_prompt_titles_are_truncated() {
        let title = title_from_prompt(&"x".repeat(200)).unwrap();
        assert_eq!(title.chars().count(), TITLE_MAX_CHARS + 1);
        assert!(title.ends_with('…'));
        assert_eq!(title_from_prompt("  \n "), None);
    }
}
