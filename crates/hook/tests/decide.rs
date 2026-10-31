//! The real shim binary against the real ingest server: a permission prompt
//! answered in the app comes out as Claude Code's hook output, and an
//! unanswered one prints nothing so Claude Code asks as usual.

use std::io::Write;
use std::process::{Command, Stdio};

use agentscope_core::ingest::{DecideRequest, IngestServer};
use agentscope_core::Decision;
use tokio::sync::mpsc;

const PAYLOAD: &str = r#"{"session_id":"s1","hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{"command":"npm publish"},"cwd":"/w"}"#;

fn run_shim(data_dir: &std::path::Path) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentscope-hook"))
        .args([
            "--harness",
            "claude-code",
            "--event",
            "PermissionRequest",
            "--decide",
        ])
        .env("AGENTSCOPE_DATA_DIR", data_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(PAYLOAD.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn answers_reach_claude_code_and_silence_lets_it_ask() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, _rx) = mpsc::channel(8);
    let (decide_tx, mut decide_rx) = mpsc::channel::<DecideRequest>(8);
    let server = IngestServer::start(dir.path(), tx, decide_tx)
        .await
        .unwrap();

    let app = tokio::spawn(async move {
        let first = decide_rx.recv().await.unwrap();
        assert_eq!(first.event.payload["tool_input"]["command"], "npm publish");
        first
            .reply
            .send(Some(Decision::Deny {
                message: "Not today".into(),
            }))
            .unwrap();
        // The second prompt is let go without an answer.
        drop(decide_rx.recv().await.unwrap());
    });

    let path = dir.path().to_owned();
    let denied = tokio::task::spawn_blocking(move || run_shim(&path))
        .await
        .unwrap();
    assert!(denied.status.success());
    let out: serde_json::Value = serde_json::from_slice(&denied.stdout).unwrap();
    assert_eq!(
        out,
        serde_json::json!({ "hookSpecificOutput": {
            "hookEventName": "PermissionRequest",
            "decision": { "behavior": "deny", "message": "Not today" }
        }})
    );

    let path = dir.path().to_owned();
    let silent = tokio::task::spawn_blocking(move || run_shim(&path))
        .await
        .unwrap();
    assert!(silent.status.success());
    assert!(silent.stdout.is_empty(), "no answer prints nothing");
    app.await.unwrap();

    // With the app gone, the shim still exits 0, prints nothing, and spools.
    server.stop().await;
    let path = dir.path().to_owned();
    let offline = tokio::task::spawn_blocking(move || run_shim(&path))
        .await
        .unwrap();
    assert!(offline.status.success());
    assert!(offline.stdout.is_empty());
    assert!(dir.path().join("spool/pending.jsonl").exists());
}
