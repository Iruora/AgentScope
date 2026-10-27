use std::process::ExitCode;
use std::time::Instant;

use agentscope_hook::{deliver, parse_args, read_payload, wrap, BUDGET};

/// Always exits 0 and never writes to stdout: harnesses may treat either as a decision.
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
    if let Some(data_dir) = agentscope_protocol::data_dir() {
        deliver(&data_dir, &wrap(args, payload), deadline);
    }
    ExitCode::SUCCESS
}
