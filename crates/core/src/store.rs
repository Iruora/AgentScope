//! SQLite persistence. Entities are stored as JSON next to the columns we
//! query on, so the schema only changes when an index is needed.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub use chrono::Duration;
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::event::AgentEvent;
use crate::model::{Agent, PendingApproval, Session, SessionLabel};
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

const MIGRATIONS: &[&str] = &[
    r#"
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
"#,
    r#"
    CREATE TABLE transcript_offsets (
        path   TEXT PRIMARY KEY,
        offset INTEGER NOT NULL
    );
"#,
    // Token usage per local day, session and model. Aggregates only, so it
    // outlives session retention (see `USAGE_DAYS`).
    r#"
    CREATE TABLE usage_daily (
        day        TEXT NOT NULL,
        session_id TEXT NOT NULL,
        harness    TEXT NOT NULL,
        model      TEXT NOT NULL,
        repo       TEXT,
        input      INTEGER NOT NULL,
        output     INTEGER NOT NULL,
        cached     INTEGER NOT NULL,
        PRIMARY KEY (day, session_id, model)
    );
    CREATE INDEX usage_session ON usage_daily (session_id);
"#,
    // What the user added to a session: pin, name, tags and a note.
    r#"
    CREATE TABLE session_meta (
        session_id TEXT PRIMARY KEY,
        pinned     INTEGER NOT NULL DEFAULT 0,
        name       TEXT,
        tags       TEXT NOT NULL DEFAULT '[]',
        note       TEXT
    );
"#,
];

const PINNED: &str = "SELECT session_id FROM session_meta WHERE pinned = 1";

/// How long daily usage totals are kept, whatever the session retention.
pub const USAGE_DAYS: i64 = 90;

/// Tokens one session used with one model on one local day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UsageRow {
    /// `YYYY-MM-DD`, local time.
    pub day: String,
    pub session_id: String,
    pub harness: crate::HarnessId,
    pub model: String,
    pub repo: Option<String>,
    pub tokens: crate::model::Tokens,
}

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
            record_usage(&tx, s)?;
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

    /// Every session label.
    pub fn labels(&self) -> Result<Vec<SessionLabel>> {
        let mut stmt = self
            .conn
            .prepare("SELECT session_id, pinned, name, tags, note FROM session_meta")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, bool>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (session_id, pinned, name, tags, note) = row?;
            out.push(SessionLabel {
                session_id,
                pinned,
                name,
                tags: serde_json::from_str(&tags).unwrap_or_default(),
                note,
            });
        }
        Ok(out)
    }

    /// Saves a label, cleaned up. An empty label is removed.
    pub fn set_label(&self, label: &SessionLabel) -> Result<SessionLabel> {
        let label = label.clone().cleaned();
        if label.is_empty() {
            self.conn.execute(
                "DELETE FROM session_meta WHERE session_id = ?1",
                [&label.session_id],
            )?;
        } else {
            self.conn.execute(
                "INSERT OR REPLACE INTO session_meta (session_id, pinned, name, tags, note)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    label.session_id,
                    label.pinned,
                    label.name,
                    serde_json::to_string(&label.tags)?,
                    label.note,
                ],
            )?;
        }
        Ok(label)
    }

    /// Ids of pinned sessions, which are kept past retention.
    pub fn pinned(&self) -> Result<HashSet<String>> {
        let mut stmt = self.conn.prepare(PINNED)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Drops events and sessions older than `retention`. Sessions that are
    /// still live, or pinned, are kept however old they are.
    pub fn prune(&mut self, retention: Duration) -> Result<usize> {
        let cutoff = (Utc::now() - retention).timestamp_millis();
        let tx = self.conn.transaction()?;
        let mut removed = tx.execute(
            &format!("DELETE FROM events WHERE at_ms < ?1 AND session_id NOT IN ({PINNED})"),
            [cutoff],
        )?;
        let old = format!(
            "SELECT id FROM sessions WHERE ended_ms IS NOT NULL AND last_event_ms < ?1 \
             AND id NOT IN ({PINNED})"
        );
        removed += tx.execute(
            &format!("DELETE FROM agents WHERE session_id IN ({old})"),
            [cutoff],
        )?;
        removed += tx.execute(
            &format!("DELETE FROM approvals WHERE session_id IN ({old})"),
            [cutoff],
        )?;
        removed += tx.execute(
            &format!("DELETE FROM sessions WHERE id IN ({old})"),
            [cutoff],
        )?;
        tx.execute(
            "DELETE FROM session_meta WHERE session_id NOT IN (SELECT id FROM sessions)",
            [],
        )?;
        let usage_cutoff = (chrono::Local::now() - Duration::days(USAGE_DAYS))
            .format("%Y-%m-%d")
            .to_string();
        removed += tx.execute("DELETE FROM usage_daily WHERE day < ?1", [usage_cutoff])?;
        tx.commit()?;
        Ok(removed)
    }

    /// How far each transcript has been read.
    pub fn transcript_offsets(&self) -> Result<HashMap<PathBuf, u64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, offset FROM transcript_offsets")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        let mut out = HashMap::new();
        for row in rows {
            let (path, offset) = row?;
            out.insert(PathBuf::from(path), offset as u64);
        }
        Ok(out)
    }

    pub fn save_offset(&self, path: &Path, offset: u64) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO transcript_offsets (path, offset) VALUES (?1, ?2)",
            params![path.to_string_lossy(), offset as i64],
        )?;
        Ok(())
    }

    pub fn forget_offset(&self, path: &Path) -> Result<()> {
        self.conn.execute(
            "DELETE FROM transcript_offsets WHERE path = ?1",
            [path.to_string_lossy()],
        )?;
        Ok(())
    }

    /// Daily usage rows from `since` (`YYYY-MM-DD`, local) on.
    pub fn usage_since(&self, since: &str) -> Result<Vec<UsageRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT day, session_id, harness, model, repo, input, output, cached
             FROM usage_daily WHERE day >= ?1 ORDER BY day",
        )?;
        let rows = stmt.query_map([since], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (day, session_id, harness, model, repo, input, output, cached) = row?;
            let Ok(harness) = serde_json::from_value(serde_json::Value::String(harness)) else {
                continue;
            };
            out.push(UsageRow {
                day,
                session_id,
                harness,
                model,
                repo,
                tokens: crate::model::Tokens {
                    input: input.max(0) as u64,
                    output: output.max(0) as u64,
                    cached: Some(cached.max(0) as u64),
                },
            });
        }
        Ok(out)
    }

    pub fn event_count(&self) -> Result<u64> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))?;
        Ok(count as u64)
    }
}

/// Adds whatever the session's token totals grew by since they were last
/// recorded to today's row for its model. Totals that went down (a harness
/// reset its counter) add nothing.
fn record_usage(tx: &rusqlite::Transaction<'_>, s: &Session) -> Result<()> {
    let Some(tokens) = s.tokens else {
        return Ok(());
    };
    let (input, output, cached): (i64, i64, i64) = tx.query_row(
        "SELECT COALESCE(SUM(input), 0), COALESCE(SUM(output), 0), COALESCE(SUM(cached), 0)
         FROM usage_daily WHERE session_id = ?1",
        [&s.id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let grow = |now: u64, then: i64| (now as i64 - then).max(0);
    let delta = (
        grow(tokens.input, input),
        grow(tokens.output, output),
        grow(tokens.cached.unwrap_or(0), cached),
    );
    if delta == (0, 0, 0) {
        return Ok(());
    }
    let day = s
        .last_event_at
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d")
        .to_string();
    tx.execute(
        "INSERT INTO usage_daily (day, session_id, harness, model, repo, input, output, cached)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT (day, session_id, model) DO UPDATE SET
           input = input + excluded.input,
           output = output + excluded.output,
           cached = cached + excluded.cached,
           repo = excluded.repo",
        params![
            day,
            s.id,
            s.harness.as_str(),
            s.model.as_deref().unwrap_or("unknown"),
            s.repo.as_ref().map(|r| r.root.as_str()),
            delta.0,
            delta.1,
            delta.2,
        ],
    )?;
    Ok(())
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
    fn labels_are_cleaned_and_pinned_sessions_survive_prune() {
        let mut store = Store::open_in_memory().unwrap();
        let mut graph = Graph::new();
        let old = Utc::now() - Duration::days(30);
        for id in ["pinned", "named"] {
            apply(&mut store, &mut graph, ev(id, old, EventKind::SessionEnded));
        }
        let pinned = graph.snapshot().sessions[0].id.clone();
        let named = graph.snapshot().sessions[1].id.clone();
        let saved = store
            .set_label(&SessionLabel {
                session_id: pinned.clone(),
                pinned: true,
                name: Some("  Release prep ".into()),
                tags: vec![
                    "#Infra".into(),
                    "infra".into(),
                    " big  refactor ".into(),
                    "".into(),
                ],
                note: Some("   ".into()),
            })
            .unwrap();
        assert_eq!(saved.name.as_deref(), Some("Release prep"));
        assert_eq!(saved.tags, ["infra", "big-refactor"]);
        assert_eq!(saved.note, None);
        store
            .set_label(&SessionLabel {
                session_id: named.clone(),
                name: Some("Gone soon".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(store.labels().unwrap().len(), 2);

        store.prune(Duration::days(7)).unwrap();
        let left: Vec<_> = store
            .load_graph()
            .unwrap()
            .snapshot()
            .sessions
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(left, std::slice::from_ref(&pinned));
        assert_eq!(
            store.labels().unwrap().len(),
            1,
            "labels of pruned sessions go too"
        );
        assert!(store.pinned().unwrap().contains(&pinned));

        store
            .set_label(&SessionLabel {
                session_id: pinned,
                ..Default::default()
            })
            .unwrap();
        assert!(
            store.labels().unwrap().is_empty(),
            "an empty label is removed"
        );
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
    fn offsets_round_trip() {
        let store = Store::open_in_memory().unwrap();
        let path = Path::new("/t/a.jsonl");
        store.save_offset(path, 10).unwrap();
        store.save_offset(path, 42).unwrap();
        assert_eq!(store.transcript_offsets().unwrap()[path], 42);
        store.forget_offset(path).unwrap();
        assert!(store.transcript_offsets().unwrap().is_empty());
    }

    #[test]
    fn migrations_are_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        Store::open(&path).unwrap();
        Store::open(&path).unwrap();
    }

    #[test]
    fn usage_records_growth_per_day_and_model() {
        let mut store = Store::open_in_memory().unwrap();
        let mut graph = Graph::new();
        let now = Utc::now();
        let used = |input, output| EventKind::TokensUsed {
            tokens: crate::model::Tokens {
                input,
                output,
                cached: Some(5),
            },
        };
        apply(&mut store, &mut graph, ev("s1", now, used(100, 10)));
        apply(&mut store, &mut graph, ev("s1", now, used(50, 5)));
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let rows = store.usage_since(&today).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].session_id, "codex:s1");
        assert_eq!(rows[0].model, "unknown");
        assert_eq!(
            (
                rows[0].tokens.input,
                rows[0].tokens.output,
                rows[0].tokens.cached
            ),
            (150, 15, Some(10))
        );
        // Saving again without new tokens adds nothing.
        let changes = Changes {
            sessions: ["codex:s1".to_owned()].into(),
            ..Changes::default()
        };
        store.save(&graph, &changes).unwrap();
        assert_eq!(store.usage_since(&today).unwrap()[0].tokens.input, 150);
    }
}
