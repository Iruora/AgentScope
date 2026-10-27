//! SQLite persistence. Entities are stored as JSON next to the columns we
//! query on, so the schema only changes when an index is needed.

use std::path::Path;

pub use chrono::Duration;
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::event::AgentEvent;
use crate::model::{Agent, PendingApproval, Session};
use crate::reducer::{Changes, Graph};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("database schema version {0} is newer than this app supports")]
    TooNew(i64),
}

pub type Result<T> = std::result::Result<T, StoreError>;

const MIGRATIONS: &[&str] = &[r#"
    CREATE TABLE sessions (
        id            TEXT PRIMARY KEY,
        harness       TEXT NOT NULL,
        last_event_ms INTEGER NOT NULL,
        ended_ms      INTEGER,
        json          TEXT NOT NULL
    );
    CREATE INDEX sessions_last_event ON sessions (last_event_ms);

    CREATE TABLE agents (
        id         TEXT PRIMARY KEY,
        session_id TEXT NOT NULL,
        json       TEXT NOT NULL
    );
    CREATE INDEX agents_session ON agents (session_id);

    CREATE TABLE approvals (
        id         TEXT PRIMARY KEY,
        session_id TEXT NOT NULL,
        requested_ms INTEGER NOT NULL,
        json       TEXT NOT NULL
    );
    CREATE INDEX approvals_session ON approvals (session_id);

    CREATE TABLE events (
        seq        INTEGER PRIMARY KEY AUTOINCREMENT,
        at_ms      INTEGER NOT NULL,
        session_id TEXT NOT NULL,
        source     TEXT NOT NULL,
        kind       TEXT NOT NULL,
        json       TEXT NOT NULL
    );
    CREATE INDEX events_at ON events (at_ms);
    CREATE INDEX events_session ON events (session_id, seq);
"#];

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > MIGRATIONS.len() as i64 {
            return Err(StoreError::TooNew(version));
        }
        let tx = conn.transaction()?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        }
        tx.commit()?;
        Ok(Self { conn })
    }

    pub fn append_event(&self, ev: &AgentEvent) -> Result<()> {
        self.conn.execute(
            "INSERT INTO events (at_ms, session_id, source, kind, json) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                ev.at.timestamp_millis(),
                Session::key(ev.harness, &ev.session),
                to_tag(&ev.source)?,
                ev.kind.name(),
                serde_json::to_string(ev)?,
            ],
        )?;
        Ok(())
    }

    /// Writes the entities in `changes` as they now are in `graph`.
    pub fn save(&mut self, graph: &Graph, changes: &Changes) -> Result<()> {
        let tx = self.conn.transaction()?;
        for s in changes.sessions.iter().filter_map(|id| graph.session(id)) {
            tx.execute(
                "INSERT OR REPLACE INTO sessions (id, harness, last_event_ms, ended_ms, json)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    s.id,
                    s.harness.as_str(),
                    s.last_event_at.timestamp_millis(),
                    s.ended_at.map(|t| t.timestamp_millis()),
                    serde_json::to_string(s)?,
                ],
            )?;
        }
        for a in changes.agents.iter().filter_map(|id| graph.agent(id)) {
            tx.execute(
                "INSERT OR REPLACE INTO agents (id, session_id, json) VALUES (?1, ?2, ?3)",
                params![a.id, a.session_id, serde_json::to_string(a)?],
            )?;
        }
        for a in changes.approvals.iter().filter_map(|id| graph.approval(id)) {
            tx.execute(
                "INSERT OR REPLACE INTO approvals (id, session_id, requested_ms, json)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    a.id,
                    a.session_id,
                    a.requested_at.timestamp_millis(),
                    serde_json::to_string(a)?
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Rebuilds the live graph from what was persisted.
    pub fn load_graph(&self) -> Result<Graph> {
        let sessions: Vec<Session> = self.load_all("SELECT json FROM sessions")?;
        let agents: Vec<Agent> = self.load_all("SELECT json FROM agents")?;
        let approvals: Vec<PendingApproval> = self.load_all("SELECT json FROM approvals")?;
        Ok(Graph::from_parts(sessions, agents, approvals))
    }

    fn load_all<T: DeserializeOwned>(&self, sql: &str) -> Result<Vec<T>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for json in rows {
            out.push(serde_json::from_str(&json?)?);
        }
        Ok(out)
    }

    /// Recent events of one session, oldest first.
    pub fn session_events(&self, session_id: &str, limit: u32) -> Result<Vec<AgentEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT json FROM (SELECT seq, json FROM events WHERE session_id = ?1
             ORDER BY seq DESC LIMIT ?2) ORDER BY seq",
        )?;
        let rows = stmt.query_map(params![session_id, limit], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for json in rows {
            out.push(serde_json::from_str(&json?)?);
        }
        Ok(out)
    }

    /// Drops events and sessions older than `retention`. Sessions that are
    /// still live are kept however old they are.
    pub fn prune(&mut self, retention: Duration) -> Result<usize> {
        let cutoff = (Utc::now() - retention).timestamp_millis();
        let tx = self.conn.transaction()?;
        let mut removed = tx.execute("DELETE FROM events WHERE at_ms < ?1", [cutoff])?;
        let old = "SELECT id FROM sessions WHERE ended_ms IS NOT NULL AND last_event_ms < ?1";
        removed += tx.execute(
            &format!("DELETE FROM agents WHERE session_id IN ({old})"),
            [cutoff],
        )?;
        removed += tx.execute(
            &format!("DELETE FROM approvals WHERE session_id IN ({old})"),
            [cutoff],
        )?;
        removed += tx.execute(
            "DELETE FROM sessions WHERE ended_ms IS NOT NULL AND last_event_ms < ?1",
            [cutoff],
        )?;
        tx.commit()?;
        Ok(removed)
    }

    pub fn event_count(&self) -> Result<u64> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))?;
        Ok(count as u64)
    }
}

/// The serde tag of a unit enum variant, e.g. `EventSource::Hook` → `"hook"`.
fn to_tag<T: Serialize>(value: &T) -> Result<String> {
    Ok(serde_json::to_value(value)?
        .as_str()
        .unwrap_or_default()
        .to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{EventKind, EventSource};
    use crate::model::{AgentState, Confidence};
    use crate::HarnessId;

    fn ev(session: &str, at: chrono::DateTime<Utc>, kind: EventKind) -> AgentEvent {
        AgentEvent {
            source: EventSource::Hook,
            at,
            harness: HarnessId::Codex,
            session: session.into(),
            agent: None,
            dedupe_key: None,
            kind,
        }
    }

    fn apply(store: &mut Store, graph: &mut Graph, ev: AgentEvent) {
        let changes = graph.apply(&ev);
        store.append_event(&ev).unwrap();
        store.save(graph, &changes).unwrap();
    }

    #[test]
    fn graph_survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agentscope.db");
        let now = Utc::now();
        {
            let mut store = Store::open(&path).unwrap();
            let mut graph = Graph::new();
            apply(
                &mut store,
                &mut graph,
                ev(
                    "s1",
                    now,
                    EventKind::PromptSubmitted {
                        prompt: "hi".into(),
                    },
                ),
            );
            apply(
                &mut store,
                &mut graph,
                ev(
                    "s1",
                    now,
                    EventKind::ApprovalRequested {
                        approval_id: "c1".into(),
                        tool: "shell".into(),
                        summary: "shell: ls".into(),
                        detail: None,
                        confidence: Confidence::Confirmed,
                    },
                ),
            );
        }
        let store = Store::open(&path).unwrap();
        let snap = store.load_graph().unwrap().snapshot();
        assert_eq!(snap.sessions.len(), 1);
        assert_eq!(snap.sessions[0].state, AgentState::WaitingApproval);
        assert_eq!(snap.approvals.len(), 1);
        assert_eq!(store.session_events("codex:s1", 10).unwrap().len(), 2);
    }

    #[test]
    fn prune_keeps_live_sessions() {
        let mut store = Store::open_in_memory().unwrap();
        let mut graph = Graph::new();
        let old = Utc::now() - Duration::days(30);
        apply(
            &mut store,
            &mut graph,
            ev("live", old, EventKind::TurnCompleted),
        );
        apply(
            &mut store,
            &mut graph,
            ev("done", old, EventKind::SessionEnded),
        );
        apply(
            &mut store,
            &mut graph,
            ev("recent", Utc::now(), EventKind::SessionEnded),
        );

        store.prune(Duration::days(7)).unwrap();
        assert_eq!(store.event_count().unwrap(), 1);
        let mut ids: Vec<_> = store
            .load_graph()
            .unwrap()
            .snapshot()
            .sessions
            .into_iter()
            .map(|s| s.id)
            .collect();
        ids.sort();
        assert_eq!(ids, ["codex:live", "codex:recent"]);
    }

    #[test]
    fn migrations_are_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        Store::open(&path).unwrap();
        Store::open(&path).unwrap();
    }
}
