use std::process::ExitCode;

/// Always exits 0. Writes to stdout only in `--decide` mode, and only when the
/// user answered in AgentScope: harnesses treat stdout as a decision.
fn main() -> ExitCode {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    if raw_args.iter().any(|a| a == "--version") {
        eprintln!("agentscope-hook {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    agentscope_hook::run(
        raw_args,
        std::io::stdin().lock(),
        &mut std::io::stdout().lock(),
        agentscope_protocol::data_dir(),
    );
    ExitCode::SUCCESS
}
