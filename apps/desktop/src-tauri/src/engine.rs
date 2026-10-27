//! Glue between the signal sources (hooks, transcripts, scans, the process
//! table), the adapters, the reducer, and the store.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

use agentscope_adapters::{
    Adapter, Detection, Diff, HookPlan, HookStatus, ProcessInfo, TranscriptFile,
};
use agentscope_core::store::{Duration, StoreError};
use agentscope_core::tail::Tailer;
use agentscope_core::{
    AgentEvent, AgentState, Changes, EventKind, EventSource, Graph, HarnessId, RawHookEvent,
    Session, Snapshot, Store,
};
use chrono::Utc;

use crate::procs::Procs;

/// Default history retention (spec section 8).
pub const DEFAULT_RETENTION_DAYS: u32 = 7;

/// The retention choices Settings offers are clamped to this range.
const RETENTION_DAYS_RANGE: std::ops::RangeInclusive<u32> = 1..=90;

/// A session with no known process and no event for this long is over.
const STALE_AFTER_MINUTES: i64 = 10;

/// Transcript events older than this are history: they never notify.
const LIVE_WINDOW_SECONDS: i64 = 120;

/// A shell or MCP call with no result for this long is probably waiting on
/// a prompt the harness does not report (Cursor).
const POSSIBLE_APPROVAL_AFTER_SECONDS: i64 = 6;

/// How deep transcript roots are walked when backfilling.
const MAX_WALK_DEPTH: usize = 5;

pub struct Engine {
    adapters: HashMap<HarnessId, Box<dyn Adapter>>,
    inner: Mutex<Inner>,
    procs: Mutex<Procs>,
    retention_days: AtomicU32,
}

struct Inner {
    graph: Graph,
    store: Store,
    tailer: Tailer,
}

/// A session that just started needing the user.
#[derive(Debug, Clone, PartialEq)]
pub struct Alert {
    pub session_id: String,
    pub state: AgentState,
    pub title: String,
    pub body: String,
}

/// What applying a batch of events did.
#[derive(Debug, Default)]
pub struct Outcome {
    pub changed: bool,
    pub alerts: Vec<Alert>,
}

impl Outcome {
    pub fn merge(&mut self, other: Outcome) {
        self.changed |= other.changed;
        self.alerts.extend(other.alerts);
    }
}

/// Where a session's agent runs, for "Go to agent".
pub struct AgentLocation {
    pub cwd: String,
    /// The harness process and its ancestors, nearest first. Empty when the
    /// process is unknown or gone.
    pub chain: Vec<ProcessInfo>,
    /// Executable paths of `chain`; macOS finds the app bundle with them.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub exes: Vec<Option<PathBuf>>,
}

impl Engine {
    pub fn open(db_path: &Path, retention_days: u32) -> Result<Self, StoreError> {
        Self::with_adapters(db_path, agentscope_adapters::all(), retention_days)
    }

    pub fn with_adapters(
        db_path: &Path,
        adapters: Vec<Box<dyn Adapter>>,
        retention_days: u32,
    ) -> Result<Self, StoreError> {
        let retention_days =
            retention_days.clamp(*RETENTION_DAYS_RANGE.start(), *RETENTION_DAYS_RANGE.end());
        let mut store = Store::open(db_path)?;
        let pruned = store.prune(Duration::days(retention_days.into()))?;
        tracing::info!(pruned, "store opened");
        let graph = store.load_graph()?;
        let tailer = Tailer::new(store.transcript_offsets()?);
        Ok(Self {
            adapters: adapters.into_iter().map(|a| (a.id(), a)).collect(),
            inner: Mutex::new(Inner {
                graph,
                store,
                tailer,
            }),
            procs: Mutex::new(Procs::new()),
            retention_days: AtomicU32::new(retention_days),
        })
    }

    fn retention(&self) -> Duration {
        Duration::days(self.retention_days.load(Ordering::Relaxed).into())
    }

    /// Changes how long ended sessions are kept, and drops what falls outside
    /// the new window.
    pub fn set_retention(&self, days: u32) -> Result<(), StoreError> {
        let days = days.clamp(*RETENTION_DAYS_RANGE.start(), *RETENTION_DAYS_RANGE.end());
        self.retention_days.store(days, Ordering::Relaxed);
        self.prune()
    }

    /// Drops events and ended sessions older than the retention window, from
    /// the store and from the graph the UI reads.
    pub fn prune(&self) -> Result<(), StoreError> {
        let retention = self.retention();
        let mut inner = self.lock();
        let pruned = inner.store.prune(retention)?;
        if pruned > 0 {
            inner.graph.forget_ended_before(Utc::now() - retention);
        }
        Ok(())
    }

    pub fn snapshot(&self) -> Snapshot {
        self.lock().graph.snapshot()
    }

    pub fn session(&self, id: &str) -> Option<Session> {
        self.lock().graph.session(id).cloned()
    }

    pub fn session_events(&self, id: &str, limit: u32) -> Result<Vec<AgentEvent>, StoreError> {
        self.lock().store.session_events(id, limit)
    }

    pub fn adapter(&self, id: HarnessId) -> Option<&dyn Adapter> {
        self.adapters.get(&id).map(|a| a.as_ref())
    }

    pub fn detections(&self) -> Vec<Detection> {
        HarnessId::ALL
            .iter()
            .filter_map(|id| self.adapters.get(id))
            .map(|a| a.detect())
            .collect()
    }

    pub fn hooks_status(&self, id: HarnessId, shim: &Path) -> HookStatus {
        self.adapter(id)
            .map_or(HookStatus::NotInstalled, |a| a.hooks_status(shim))
    }

    pub fn set_hooks(
        &self,
        id: HarnessId,
        shim: &Path,
        connect: bool,
        dry_run: bool,
    ) -> agentscope_adapters::Result<Diff> {
        let adapter = self
            .adapter(id)
            .ok_or(agentscope_adapters::AdapterError::NotImplemented(
                "this harness",
            ))?;
        if connect {
            adapter.install_hooks(&HookPlan {
                shim_path: shim.to_owned(),
                dry_run,
            })
        } else {
            adapter.uninstall_hooks(dry_run)
        }
    }

    /// Normalizes, applies, and persists one hook event.
    pub fn ingest(&self, raw: &RawHookEvent) -> Outcome {
        let Some(adapter) = self.adapter(raw.harness) else {
            return Outcome::default();
        };
        let mut events = adapter.normalize_hook(raw);
        let Some(first) = events.first().cloned() else {
            tracing::debug!(harness = %raw.harness, event = %raw.event, "hook event not handled");
            return Outcome::default();
        };
        // Learn the harness pid, used for liveness and "Go to agent".
        if let Some(ppid) = raw.ppid {
            let session_id = Session::key(first.harness, &first.session);
            let needs_pid = {
                let inner = self.lock();
                let session = inner.graph.session(&session_id);
                session.is_none_or(|s| s.pid.is_none() || s.ended_at.is_some())
                    || matches!(first.kind, EventKind::SessionStarted { .. })
            };
            if needs_pid {
                if let Some(pid) = self.procs().harness_pid(ppid, adapter) {
                    events.push(AgentEvent {
                        agent: None,
                        dedupe_key: None,
                        kind: EventKind::Heartbeat { pid },
                        ..first
                    });
                }
            }
        }
        self.apply(&events)
    }

    /// Reads what was appended to a transcript since last time.
    pub fn tail(&self, path: &Path) -> Outcome {
        let Some(adapter) = self.adapter_for_transcript(path) else {
            return Outcome::default();
        };
        let mut outcome = Outcome::default();
        loop {
            let read = self.lock().tailer.read(path);
            let chunk = match read {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                Err(err) => {
                    if err.kind() == std::io::ErrorKind::NotFound {
                        let mut inner = self.lock();
                        inner.tailer.forget(path);
                        let _ = inner.store.forget_offset(path);
                    }
                    break;
                }
            };
            let file = TranscriptFile {
                path: path.to_owned(),
                offset: chunk.offset,
            };
            let events = adapter.parse_transcript_chunk(&file, &chunk.bytes);
            outcome.merge(self.apply(&events));
            let inner = self.lock();
            let offset = inner.tailer.offset(path);
            if let Err(err) = inner.store.save_offset(path, offset) {
                tracing::error!(%err, "failed to save transcript offset");
            }
            if !chunk.more {
                break;
            }
        }
        outcome
    }

    /// Every transcript root, for the file watcher.
    pub fn transcript_roots(&self) -> Vec<agentscope_adapters::WatchRoot> {
        self.adapters
            .values()
            .flat_map(|a| a.transcript_roots())
            .collect()
    }

    /// Reads transcripts changed since `since`. Files last written before the
    /// retention window are skipped, and only their future lines are read.
    pub fn backfill(&self, since: Option<std::time::SystemTime>) -> Outcome {
        let cutoff = std::time::SystemTime::now() - self.retention().to_std().unwrap_or_default();
        let mut files = Vec::new();
        for root in self.transcript_roots() {
            walk(&root.path, root.suffix, MAX_WALK_DEPTH, &mut files);
        }
        files.sort_by_key(|(_, modified, _)| *modified);
        let mut outcome = Outcome::default();
        for (path, modified, len) in files {
            if since.is_some_and(|s| modified < s) {
                continue;
            }
            if modified < cutoff {
                let mut inner = self.lock();
                if inner.tailer.offset(&path) == 0 && len > 0 {
                    inner.tailer.set_offset(&path, len);
                    let _ = inner.store.save_offset(&path, len);
                }
                continue;
            }
            outcome.merge(self.tail(&path));
        }
        outcome
    }

    fn adapter_for_transcript(&self, path: &Path) -> Option<&dyn Adapter> {
        self.adapters.values().map(|a| a.as_ref()).find(|a| {
            a.transcript_roots().iter().any(|root| {
                path.starts_with(&root.path)
                    && path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.ends_with(root.suffix))
            })
        })
    }

    /// Periodic work: adapter scans, process liveness, and repo branches.
    pub fn sweep(&self) -> Outcome {
        let now = Utc::now();
        let mut procs = self.procs();
        procs.refresh();

        let live: Vec<Session> = self
            .lock()
            .graph
            .sessions()
            .filter(|s| s.ended_at.is_none())
            .cloned()
            .collect();
        let known: HashMap<String, (Option<u32>, bool, String)> = self
            .lock()
            .graph
            .sessions()
            .map(|s| (s.id.clone(), (s.pid, s.ended_at.is_some(), s.cwd.clone())))
            .collect();

        let mut events = Vec::new();
        for adapter in self.adapters.values() {
            for ev in adapter.scan(now - Duration::days(1)) {
                let id = Session::key(ev.harness, &ev.session);
                let current = known.get(&id);
                let keep = match &ev.kind {
                    EventKind::Heartbeat { pid } => {
                        procs.is_agent(*pid, adapter.as_ref())
                            && current.is_none_or(|(known_pid, ended, _)| {
                                *known_pid != Some(*pid) || *ended
                            })
                    }
                    EventKind::SessionInfo { cwd: Some(cwd), .. } => {
                        current.is_none_or(|(_, _, known_cwd)| known_cwd != cwd)
                    }
                    _ => true,
                };
                if keep {
                    events.push(ev);
                }
            }
        }

        let stale = now - Duration::minutes(STALE_AFTER_MINUTES);
        let scanned: HashSet<String> = events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::Heartbeat { .. }))
            .map(|e| Session::key(e.harness, &e.session))
            .collect();
        for session in &live {
            let Some(adapter) = self.adapter(session.harness) else {
                continue;
            };
            let alive = match session.pid {
                Some(pid) => procs.is_agent(pid, adapter),
                None => {
                    session.last_event_at > stale
                        || scanned.contains(&session.id)
                        || (!session.cwd.is_empty()
                            && procs
                                .find_by_cwd(Path::new(&session.cwd), adapter)
                                .is_some())
                }
            };
            if !alive {
                // A process that just exited ended now; a session that went
                // quiet ended with its last event, which keeps backfilled
                // sessions on the right day in History.
                let at = if session.pid.is_some() {
                    now
                } else {
                    session.last_event_at.min(now)
                };
                events.push(AgentEvent {
                    source: EventSource::Process,
                    at,
                    harness: session.harness,
                    session: session.native_id.clone(),
                    agent: None,
                    dedupe_key: None,
                    kind: EventKind::SessionEnded,
                });
            }
        }
        drop(procs);

        events.extend(
            self.lock()
                .graph
                .due_possible_approvals(now, Duration::seconds(POSSIBLE_APPROVAL_AFTER_SECONDS)),
        );
        let mut outcome = self.apply(&events);

        // Branches change under a running agent, so re-read them every sweep.
        let mut inner = self.lock();
        let mut changes = Changes::default();
        for session in live.iter().filter(|s| !s.cwd.is_empty()) {
            let repo = agentscope_core::repo::repo_for(Path::new(&session.cwd));
            if inner.graph.set_repo(&session.id, repo) {
                changes.sessions.insert(session.id.clone());
            }
        }
        if !changes.is_empty() {
            let Inner { graph, store, .. } = &mut *inner;
            if let Err(err) = store.save(graph, &changes) {
                tracing::error!(%err, "failed to save repos");
            }
            outcome.changed = true;
        }
        outcome
    }

    /// The process tree of a session's agent.
    pub fn locate(&self, session_id: &str) -> Option<AgentLocation> {
        let session = self.session(session_id)?;
        let adapter = self.adapter(session.harness)?;
        let mut procs = self.procs();
        procs.refresh();
        let pid = session
            .pid
            .filter(|pid| procs.is_agent(*pid, adapter))
            .or_else(|| {
                (!session.cwd.is_empty())
                    .then(|| procs.find_by_cwd(Path::new(&session.cwd), adapter))
                    .flatten()
            });
        let chain = pid.map(|pid| procs.ancestors(pid)).unwrap_or_default();
        let exes = chain.iter().map(|p| procs.exe(p.pid)).collect();
        Some(AgentLocation {
            cwd: session.cwd,
            chain,
            exes,
        })
    }

    /// Applies events in order, persists what changed, and reports sessions
    /// that just started needing the user.
    fn apply(&self, events: &[AgentEvent]) -> Outcome {
        if events.is_empty() {
            return Outcome::default();
        }
        let now = Utc::now();
        let mut inner = self.lock();
        let Inner { graph, store, .. } = &mut *inner;
        let mut changes = Changes::default();
        let mut before: HashMap<String, AgentState> = HashMap::new();
        let mut live: HashSet<String> = HashSet::new();
        for ev in events {
            let id = Session::key(ev.harness, &ev.session);
            before
                .entry(id.clone())
                .or_insert_with(|| graph.session(&id).map(|s| s.state).unwrap_or_default());
            let applied = graph.apply(ev);
            if applied.is_empty() {
                continue;
            }
            if ev.source == EventSource::Hook || (now - ev.at).num_seconds() < LIVE_WINDOW_SECONDS {
                live.insert(id);
            }
            // Heartbeats only refresh liveness; they would flood the timeline.
            if !matches!(ev.kind, EventKind::Heartbeat { .. }) {
                if let Err(err) = store.append_event(ev) {
                    tracing::error!(%err, "failed to store event");
                }
            }
            changes.extend(applied);
        }
        if let Err(err) = store.save(graph, &changes) {
            tracing::error!(%err, "failed to save graph");
        }

        let mut alerts = Vec::new();
        for id in live {
            let Some(session) = graph.session(&id) else {
                continue;
            };
            if session.ended_at.is_some()
                || !session.state.needs_user()
                || before.get(&id) == Some(&session.state)
            {
                continue;
            }
            alerts.push(alert_for(graph, session));
        }
        Outcome {
            changed: !changes.is_empty(),
            alerts,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panic while holding the lock leaves the graph usable; keep serving it.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn procs(&self) -> MutexGuard<'_, Procs> {
        self.procs.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn alert_for(graph: &Graph, session: &Session) -> Alert {
    let name = session
        .title
        .clone()
        .unwrap_or_else(|| crate::focus::display_dir(&session.cwd));
    let body = match session.state {
        AgentState::WaitingApproval | AgentState::MaybeWaiting => graph
            .snapshot()
            .approvals
            .into_iter()
            .rev()
            .find(|a| a.session_id == session.id)
            .map(|a| format!("Wants to run {}", a.summary))
            .unwrap_or_else(|| "Needs your approval".into()),
        _ => "Waiting for your input".into(),
    };
    Alert {
        session_id: session.id.clone(),
        state: session.state,
        title: name,
        body,
    }
}

/// Collects `(path, modified, len)` of files ending in `suffix`.
fn walk(
    dir: &Path,
    suffix: &str,
    depth: usize,
    out: &mut Vec<(PathBuf, std::time::SystemTime, u64)>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        let path = entry.path();
        if meta.is_dir() {
            if depth > 0 {
                walk(&path, suffix, depth - 1, out);
            }
        } else if path.to_str().is_some_and(|p| p.ends_with(suffix)) {
            let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
            out.push((path, modified, meta.len()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentscope_adapters::ClaudeCode;
    use agentscope_protocol::PROTOCOL_VERSION;

    const SESSION: &str = "5f0c7a52-1d1e-4c3b-9d7e-0b8e6f1a2c11";

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/claude-code/2.1")
    }

    fn engine(config: &Path, db: &Path) -> Engine {
        Engine::with_adapters(
            db,
            vec![Box::new(ClaudeCode::with_config_dir(config))],
            DEFAULT_RETENTION_DAYS,
        )
        .unwrap()
    }

    fn hook(name: &str) -> RawHookEvent {
        let payload = std::fs::read(fixtures().join(format!("hooks/{name}.json"))).unwrap();
        RawHookEvent {
            v: PROTOCOL_VERSION,
            harness: HarnessId::ClaudeCode,
            event: name.into(),
            received_at: Utc::now().timestamp_millis() as u64,
            pid: None,
            // No such process, so no pid is learned from it.
            ppid: Some(u32::MAX - 7),
            payload: serde_json::from_slice(&payload).unwrap(),
        }
    }

    #[test]
    fn hooks_alert_once_when_a_session_starts_waiting() {
        let dir = tempfile::tempdir().unwrap();
        let engine = engine(dir.path(), &dir.path().join("db"));
        assert!(engine.ingest(&hook("UserPromptSubmit")).alerts.is_empty());

        let outcome = engine.ingest(&hook("PermissionRequest"));
        assert!(outcome.changed);
        assert_eq!(outcome.alerts.len(), 1);
        let alert = &outcome.alerts[0];
        assert_eq!(alert.state, AgentState::WaitingApproval);
        assert_eq!(alert.title, "Fix the failing checkout tests");
        assert_eq!(
            alert.body,
            "Wants to run Bash: STRIPE_SECRET_KEY=[redacted] npm run e2e"
        );

        let session = engine.session(&format!("claude-code:{SESSION}")).unwrap();
        assert_eq!(session.pid, None);
        let events = engine.session_events(&session.id, 10).unwrap();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn transcripts_are_tailed_once_and_stale_sessions_end() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("projects/-work-shop");
        std::fs::create_dir_all(&project).unwrap();
        let src = fixtures().join(format!("projects/-work-shop/{SESSION}.jsonl"));
        let main = project.join(format!("{SESSION}.jsonl"));
        std::fs::copy(&src, &main).unwrap();
        let db = dir.path().join("db");

        {
            let engine = engine(dir.path(), &db);
            let outcome = engine.backfill(None);
            assert!(outcome.changed);
            assert!(outcome.alerts.is_empty(), "history never notifies");
            let session = engine.session(&format!("claude-code:{SESSION}")).unwrap();
            assert_eq!(session.title.as_deref(), Some("Fix failing checkout tests"));
            assert!(session.ended_at.is_none());

            // The fixture's last event is old and no process runs in its cwd.
            engine.sweep();
            let session = engine.session(&session.id).unwrap();
            assert_eq!(session.state, AgentState::Ended);
        }

        // A restart resumes from the saved offset instead of replaying.
        let engine = engine(dir.path(), &db);
        assert!(!engine.tail(&main).changed);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&main)
            .unwrap();
        let prompt = serde_json::json!({
            "type": "user", "sessionId": SESSION, "cwd": "/work/shop", "promptId": "p9",
            "timestamp": Utc::now().to_rfc3339(),
            "message": { "role": "user", "content": "Now update the changelog" }
        });
        std::io::Write::write_all(&mut file, format!("{prompt}\n").as_bytes()).unwrap();
        assert!(engine.tail(&main).changed);
        let session = engine.session(&format!("claude-code:{SESSION}")).unwrap();
        assert!(
            session.ended_at.is_none(),
            "new activity reopens the session"
        );
        assert_eq!(session.state, AgentState::Thinking);
    }

    #[test]
    fn old_transcripts_are_skipped_but_followed() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("projects/-old");
        std::fs::create_dir_all(&project).unwrap();
        let path = project.join("old.jsonl");
        std::fs::write(&path, "{\"type\":\"user\"}\n").unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(30 * 86_400);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(old)
            .unwrap();

        let engine = engine(dir.path(), &dir.path().join("db"));
        engine.backfill(None);
        assert_eq!(engine.lock().tailer.offset(&path), 16);
        assert!(engine.snapshot().sessions.is_empty());
    }
}
