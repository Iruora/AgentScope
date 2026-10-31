//! `agentscope-hook`: forwards one hook payload to the AgentScope app.
//!
//! Harnesses run this on every hook event, so it must never slow down or break
//! an agent: it has a hard time budget, never writes to stdout, and the binary
//! always exits 0. When the app is not reachable the event goes to a bounded
//! spool that the app drains on its next start.

use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::time::{Duration, Instant};

use agentscope_protocol::{
    ingest_info_path, now_ms, spool_dir, DecideReply, Decision, HarnessId, IngestInfo,
    RawHookEvent, DECIDE_MAX_WAIT_SECS, DECIDE_PATH, INGEST_PATH, MAX_PAYLOAD_BYTES,
    PROTOCOL_VERSION, SPOOL_MAX_BYTES, TOKEN_HEADER,
};

/// Total time the shim may spend delivering an event.
pub const BUDGET: Duration = Duration::from_millis(300);

#[derive(Debug, PartialEq, Eq)]
pub struct Args {
    pub harness: HarnessId,
    pub event: String,
    /// Wait for the user's answer from AgentScope and print it for the
    /// harness (opt-in approvals). Without it the shim never writes to stdout.
    pub decide: bool,
}

pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut harness = None;
    let mut event = None;
    let mut decide = false;
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--harness" => harness = it.next(),
            "--event" => event = it.next(),
            "--decide" => decide = true,
            other => return Err(format!("unexpected argument `{other}`")),
        }
    }
    let harness = harness
        .ok_or("missing --harness")?
        .parse::<HarnessId>()
        .map_err(|e| e.to_string())?;
    let event = event.filter(|e| !e.is_empty()).ok_or("missing --event")?;
    Ok(Args {
        harness,
        event,
        decide,
    })
}

/// Reads at most [`MAX_PAYLOAD_BYTES`] from `input`. Invalid JSON is kept as a string.
pub fn read_payload(input: impl Read) -> serde_json::Value {
    let mut buf = Vec::new();
    let _ = input.take(MAX_PAYLOAD_BYTES as u64).read_to_end(&mut buf);
    match serde_json::from_slice(&buf) {
        Ok(value) => value,
        Err(_) => serde_json::Value::String(String::from_utf8_lossy(&buf).into_owned()),
    }
}

pub fn wrap(args: &Args, payload: serde_json::Value) -> RawHookEvent {
    RawHookEvent {
        v: PROTOCOL_VERSION,
        harness: args.harness,
        event: args.event.clone(),
        received_at: now_ms(),
        pid: Some(std::process::id()),
        ppid: parent_pid(),
        payload,
    }
}

#[cfg(unix)]
fn parent_pid() -> Option<u32> {
    Some(std::os::unix::process::parent_id())
}

#[cfg(windows)]
fn parent_pid() -> Option<u32> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let me = std::process::id();
    // SAFETY: plain Win32 calls on a snapshot handle we own and close.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = None;
        let mut ok = Process32FirstW(snapshot, &mut entry) != 0;
        while ok {
            if entry.th32ProcessID == me {
                found = Some(entry.th32ParentProcessID);
                break;
            }
            ok = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        found
    }
}

#[cfg(not(any(unix, windows)))]
fn parent_pid() -> Option<u32> {
    None
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Delivered,
    Spooled,
    Dropped,
}

/// Sends `event` to the app, falling back to the spool. Never panics.
pub fn deliver(data_dir: &Path, event: &RawHookEvent, deadline: Instant) -> Outcome {
    let Ok(line) = serde_json::to_vec(event) else {
        return Outcome::Dropped;
    };
    if send(data_dir, &line, deadline).is_ok() {
        return Outcome::Delivered;
    }
    match spool(data_dir, &line) {
        Ok(()) => Outcome::Spooled,
        Err(_) => Outcome::Dropped,
    }
}

fn send(data_dir: &Path, body: &[u8], deadline: Instant) -> io::Result<()> {
    let info: IngestInfo = serde_json::from_slice(&fs::read(ingest_info_path(data_dir))?)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, info.port));
    let mut stream = TcpStream::connect_timeout(&addr, remaining(deadline)?)?;
    stream.set_nodelay(true)?;
    stream.set_write_timeout(Some(remaining(deadline)?))?;
    let head = format!(
        "POST {INGEST_PATH} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{TOKEN_HEADER}: {}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        info.port,
        info.token,
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;

    stream.set_read_timeout(Some(remaining(deadline)?))?;
    let mut status = [0u8; 12];
    stream.read_exact(&mut status)?;
    // "HTTP/1.1 2xx"
    if status.starts_with(b"HTTP/1.") && status[9] == b'2' {
        Ok(())
    } else {
        Err(io::Error::other("ingest rejected the event"))
    }
}

/// Sends a permission prompt to the app and waits for the user's answer.
/// `Ok(None)` means the app saw it but has no answer (timeout, answered in
/// the terminal, or approvals turned off); `Err` means the app was not reached.
pub fn decide(
    data_dir: &Path,
    event: &RawHookEvent,
    connect_budget: Duration,
) -> io::Result<Option<Decision>> {
    let body = serde_json::to_vec(event).map_err(io::Error::other)?;
    let info: IngestInfo = serde_json::from_slice(&fs::read(ingest_info_path(data_dir))?)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, info.port));
    let mut stream = TcpStream::connect_timeout(&addr, connect_budget)?;
    stream.set_nodelay(true)?;
    stream.set_write_timeout(Some(connect_budget))?;
    let head = format!(
        "POST {DECIDE_PATH} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{TOKEN_HEADER}: {}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        info.port,
        info.token,
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&body)?;
    stream.flush()?;

    stream.set_read_timeout(Some(Duration::from_secs(DECIDE_MAX_WAIT_SECS)))?;
    let mut response = Vec::new();
    stream.take(64 * 1024).read_to_end(&mut response)?;
    parse_decide_response(&response)
}

fn parse_decide_response(response: &[u8]) -> io::Result<Option<Decision>> {
    let split = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))?;
    let (head, body) = (&response[..split], &response[split + 4..]);
    if !(head.starts_with(b"HTTP/1.") && head.get(9) == Some(&b'2')) {
        return Err(io::Error::other("the app rejected the prompt"));
    }
    let reply: DecideReply = serde_json::from_slice(body).map_err(io::Error::other)?;
    Ok(reply.decision)
}

/// What to print for the harness. Only Claude Code takes answers so far.
pub fn render_decision(harness: HarnessId, event: &str, decision: &Decision) -> Option<String> {
    match harness {
        HarnessId::ClaudeCode if event == "PermissionRequest" => Some(
            serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PermissionRequest",
                    "decision": decision,
                }
            })
            .to_string(),
        ),
        _ => None,
    }
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::from(io::ErrorKind::TimedOut))
}

/// Appends one JSON line to the spool, unless that would exceed [`SPOOL_MAX_BYTES`].
pub fn spool(data_dir: &Path, line: &[u8]) -> io::Result<()> {
    let dir = spool_dir(data_dir);
    fs::create_dir_all(&dir)?;
    let path = dir.join("pending.jsonl");
    let used: u64 = fs::read_dir(&dir)?
        .filter_map(|e| e.ok()?.metadata().ok())
        .map(|m| m.len())
        .sum();
    if used + line.len() as u64 + 1 > SPOOL_MAX_BYTES {
        return Err(io::Error::other("spool is full"));
    }
    let mut record = Vec::with_capacity(line.len() + 1);
    record.extend_from_slice(line);
    record.push(b'\n');
    // One write per record so concurrent shims append whole lines.
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(&record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::net::TcpListener;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_args() {
        let parsed = parse_args(args(&["--harness", "claude-code", "--event", "PreToolUse"]));
        assert_eq!(
            parsed.unwrap(),
            Args {
                harness: HarnessId::ClaudeCode,
                event: "PreToolUse".into(),
                decide: false,
            }
        );
        let decide = parse_args(args(&[
            "--harness",
            "claude-code",
            "--event",
            "PermissionRequest",
            "--decide",
        ]))
        .unwrap();
        assert!(decide.decide);
        assert!(parse_args(args(&["--harness", "nope", "--event", "x"])).is_err());
        assert!(parse_args(args(&["--harness", "codex"])).is_err());
        assert!(parse_args(args(&["--verbose"])).is_err());
    }

    #[test]
    fn keeps_non_json_payload_as_string() {
        assert_eq!(
            read_payload(&b"{\"a\":1}"[..]),
            serde_json::json!({ "a": 1 })
        );
        assert_eq!(
            read_payload(&b"not json"[..]),
            serde_json::json!("not json")
        );
    }

    fn event() -> RawHookEvent {
        wrap(
            &Args {
                harness: HarnessId::Codex,
                event: "Stop".into(),
                decide: false,
            },
            serde_json::json!({ "session_id": "s1" }),
        )
    }

    #[test]
    fn spools_when_app_is_not_running() {
        let dir = tempfile::tempdir().unwrap();
        let ev = event();
        assert_eq!(
            deliver(dir.path(), &ev, Instant::now() + BUDGET),
            Outcome::Spooled
        );
        let file = fs::File::open(spool_dir(dir.path()).join("pending.jsonl")).unwrap();
        let lines: Vec<_> = io::BufReader::new(file)
            .lines()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(serde_json::from_str::<RawHookEvent>(&lines[0]).unwrap(), ev);
    }

    #[test]
    fn spool_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let big = vec![b'x'; SPOOL_MAX_BYTES as usize - 10];
        spool(dir.path(), &big).unwrap();
        assert!(spool(dir.path(), b"{\"more\":true}").is_err());
    }

    #[test]
    fn delivers_to_ingest_server() {
        let dir = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let info = IngestInfo {
            v: PROTOCOL_VERSION,
            port: listener.local_addr().unwrap().port(),
            token: "t0ken".into(),
            pid: 1,
        };
        fs::write(
            ingest_info_path(dir.path()),
            serde_json::to_vec(&info).unwrap(),
        )
        .unwrap();

        let server = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut reader = io::BufReader::new(conn.try_clone().unwrap());
            let mut head = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                head.push_str(&line);
            }
            conn.write_all(b"HTTP/1.1 204 No Content\r\n\r\n").unwrap();
            head
        });

        assert_eq!(
            deliver(dir.path(), &event(), Instant::now() + BUDGET),
            Outcome::Delivered
        );
        let head = server.join().unwrap();
        assert!(head.starts_with("POST /v1/hook HTTP/1.1"));
        assert!(head.contains("x-agentscope-token: t0ken"));
        assert!(!spool_dir(dir.path()).exists());
    }

    #[test]
    fn renders_claude_code_decisions_only() {
        let deny = Decision::Deny {
            message: "Denied in AgentScope".into(),
        };
        let out = render_decision(HarnessId::ClaudeCode, "PermissionRequest", &deny).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&out).unwrap(),
            serde_json::json!({ "hookSpecificOutput": {
                "hookEventName": "PermissionRequest",
                "decision": { "behavior": "deny", "message": "Denied in AgentScope" }
            }})
        );
        assert!(render_decision(HarnessId::ClaudeCode, "PreToolUse", &deny).is_none());
        assert!(render_decision(HarnessId::Codex, "PermissionRequest", &deny).is_none());
    }

    #[test]
    fn parses_decide_responses() {
        let ok = b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n{\"decision\":{\"behavior\":\"allow\"}}";
        assert_eq!(parse_decide_response(ok).unwrap(), Some(Decision::Allow));
        let none = b"HTTP/1.1 200 OK\r\n\r\n{\"decision\":null}";
        assert_eq!(parse_decide_response(none).unwrap(), None);
        assert!(parse_decide_response(b"HTTP/1.1 401 Unauthorized\r\n\r\n").is_err());
        assert!(parse_decide_response(b"HTTP/1.1 200 OK\r\n").is_err());
    }

    #[test]
    fn decide_fails_fast_without_the_app() {
        let dir = tempfile::tempdir().unwrap();
        let ev = event();
        assert!(decide(dir.path(), &ev, BUDGET).is_err());
    }
}
