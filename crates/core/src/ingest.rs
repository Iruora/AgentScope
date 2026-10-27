//! Local ingest server for the `agentscope-hook` shim.
//!
//! Listens on `127.0.0.1` on a random port and requires the random token the
//! app writes to `ingest.json` on every start. Events that arrived while the
//! app was down are read back from the shim's spool with [`drain_spool`].

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use agentscope_protocol::{
    ingest_info_path, now_ms, spool_dir, IngestInfo, RawHookEvent, INGEST_PATH, MAX_PAYLOAD_BYTES,
    PROTOCOL_VERSION, TOKEN_HEADER,
};
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

pub struct IngestServer {
    pub port: u16,
    info_path: PathBuf,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

#[derive(Clone)]
struct Ctx {
    token: String,
    tx: mpsc::Sender<RawHookEvent>,
}

impl IngestServer {
    /// Binds the server and writes `ingest.json` so shims can find it.
    pub async fn start(data_dir: &Path, tx: mpsc::Sender<RawHookEvent>) -> io::Result<Self> {
        fs::create_dir_all(data_dir)?;
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        let token = new_token()?;
        let info_path = ingest_info_path(data_dir);
        write_private(
            &info_path,
            &serde_json::to_vec(&IngestInfo {
                v: PROTOCOL_VERSION,
                port,
                token: token.clone(),
                pid: std::process::id(),
            })?,
        )?;

        let app = Router::new()
            .route("/v1/health", get(|| async { "ok" }))
            .route(INGEST_PATH, post(receive))
            .layer(DefaultBodyLimit::max(MAX_PAYLOAD_BYTES + 64 * 1024))
            .with_state(Ctx { token, tx });
        let (shutdown, stop) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let serve = axum::serve(listener, app).with_graceful_shutdown(async {
                let _ = stop.await;
            });
            if let Err(err) = serve.await {
                tracing::error!(%err, "ingest server stopped");
            }
        });
        tracing::info!(port, "ingest server listening");
        Ok(Self {
            port,
            info_path,
            shutdown: Some(shutdown),
            task: Some(task),
        })
    }

    /// Stops accepting events and removes `ingest.json`, so shims spool instead.
    pub async fn stop(mut self) {
        let _ = fs::remove_file(&self.info_path);
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for IngestServer {
    fn drop(&mut self) {
        if self.shutdown.is_some() {
            let _ = fs::remove_file(&self.info_path);
        }
    }
}

async fn receive(
    State(ctx): State<Ctx>,
    headers: HeaderMap,
    Json(mut event): Json<RawHookEvent>,
) -> StatusCode {
    let authorized = headers
        .get(TOKEN_HEADER)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| constant_time_eq(t.as_bytes(), ctx.token.as_bytes()));
    if !authorized {
        return StatusCode::UNAUTHORIZED;
    }
    if event.received_at == 0 {
        event.received_at = now_ms();
    }
    // Never make the shim wait on the app: drop the event if the queue is full.
    match ctx.tx.try_send(event) {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(mpsc::error::TrySendError::Full(_)) => {
            tracing::warn!("ingest queue full, dropping hook event");
            StatusCode::SERVICE_UNAVAILABLE
        }
        Err(mpsc::error::TrySendError::Closed(_)) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

/// Reads and deletes everything the shim spooled while the app was not
/// running, oldest first. Lines that fail to parse are skipped.
pub fn drain_spool(data_dir: &Path) -> io::Result<Vec<RawHookEvent>> {
    let dir = spool_dir(data_dir);
    let pending = dir.join("pending.jsonl");
    // Move the file aside first so shims running right now start a new one.
    if pending.exists() {
        fs::rename(&pending, dir.join(format!("draining-{}.jsonl", now_ms())))?;
    }
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut events = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let is_draining = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("draining-") && n.ends_with(".jsonl"));
        if !is_draining {
            continue;
        }
        let file = io::BufReader::new(fs::File::open(&path)?);
        for line in file.lines() {
            match serde_json::from_str::<RawHookEvent>(&line?) {
                Ok(ev) => events.push(ev),
                Err(err) => tracing::warn!(%err, "skipping malformed spool line"),
            }
        }
        fs::remove_file(&path)?;
    }
    events.sort_by_key(|e| e.received_at);
    Ok(events)
}

fn new_token() -> io::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| io::Error::other(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Writes atomically, readable only by the current user on Unix.
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentscope_protocol::HarnessId;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn raw(event: &str, at: u64) -> RawHookEvent {
        RawHookEvent {
            v: PROTOCOL_VERSION,
            harness: HarnessId::ClaudeCode,
            event: event.into(),
            received_at: at,
            pid: None,
            ppid: None,
            payload: serde_json::json!({}),
        }
    }

    async fn post(port: u16, token: &str, body: &str) -> String {
        let mut conn = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        let req = format!(
            "POST {INGEST_PATH} HTTP/1.1\r\nHost: x\r\n{TOKEN_HEADER}: {token}\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        conn.write_all(req.as_bytes()).await.unwrap();
        let mut resp = String::new();
        conn.read_to_string(&mut resp).await.unwrap();
        resp
    }

    #[tokio::test]
    async fn accepts_events_with_the_token_only() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, mut rx) = mpsc::channel(8);
        let server = IngestServer::start(dir.path(), tx).await.unwrap();
        let info: IngestInfo =
            serde_json::from_slice(&fs::read(ingest_info_path(dir.path())).unwrap()).unwrap();
        assert_eq!(info.port, server.port);

        let body = serde_json::to_string(&raw("Stop", 5)).unwrap();
        assert!(post(server.port, "wrong", &body)
            .await
            .starts_with("HTTP/1.1 401"));
        assert!(post(server.port, &info.token, &body)
            .await
            .starts_with("HTTP/1.1 204"));
        assert_eq!(rx.recv().await.unwrap().event, "Stop");

        server.stop().await;
        assert!(!ingest_info_path(dir.path()).exists());
    }

    #[test]
    fn drains_spool_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let spool = spool_dir(dir.path());
        fs::create_dir_all(&spool).unwrap();
        let lines = [
            serde_json::to_string(&raw("B", 2)).unwrap(),
            "garbage".to_owned(),
            serde_json::to_string(&raw("A", 1)).unwrap(),
        ];
        fs::write(spool.join("pending.jsonl"), lines.join("\n") + "\n").unwrap();

        let events = drain_spool(dir.path()).unwrap();
        assert_eq!(
            events.iter().map(|e| e.event.as_str()).collect::<Vec<_>>(),
            ["A", "B"]
        );
        assert_eq!(fs::read_dir(&spool).unwrap().count(), 0);
        assert!(drain_spool(dir.path()).unwrap().is_empty());
    }
}
