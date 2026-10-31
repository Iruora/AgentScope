use std::process::ExitCode;
use std::time::Instant;

use std::io::Write;

use agentscope_hook::{
    decide, deliver, parse_args, read_payload, render_decision, spool, wrap, BUDGET,
};

/// Always exits 0. Writes to stdout only in `--decide` mode, and only when the
/// user answered in AgentScope: harnesses treat stdout as a decision.
fn main() -> ExitCode {
    let deadline = Instant::now() + BUDGET;
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    if raw_args.iter().any(|a| a == "--version") {
        eprintln!("agentscope-hook {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let args = match parse_args(raw_args) {
        Ok(args) => args,
        Err(err) => {
            eprintln!("agentscope-hook: {err}");
            eprintln!("usage: agentscope-hook --harness <claude-code|codex|copilot-cli|cursor> --event <name>");
            return ExitCode::SUCCESS;
        }
    };
    let payload = read_payload(std::io::stdin().lock());
    let Some(data_dir) = agentscope_protocol::data_dir() else {
        return ExitCode::SUCCESS;
    };
    let event = wrap(&args, payload);
    if !args.decide {
        deliver(&data_dir, &event, deadline);
        return ExitCode::SUCCESS;
    }
    match decide(&data_dir, &event, BUDGET) {
        Ok(Some(decision)) => {
            if let Some(out) = render_decision(args.harness, &args.event, &decision) {
                let mut stdout = std::io::stdout().lock();
                let _ = writeln!(stdout, "{out}");
                let _ = stdout.flush();
            }
        }
        // The harness shows its own prompt, as without AgentScope.
        Ok(None) => {}
        // The app is not running: keep the event for later, answer nothing.
        Err(_) => {
            if let Ok(line) = serde_json::to_vec(&event) {
                let _ = spool(&data_dir, &line);
            }
        }
    }
    ExitCode::SUCCESS
}
