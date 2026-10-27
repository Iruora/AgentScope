# AgentScope

Every agent. Every action. One view.

A cross-platform desktop app that shows every local AI coding agent (Claude Code, Codex CLI, GitHub Copilot CLI, Cursor), their subagents, what they are doing, and which ones are waiting for your approval.

- [v1 spec](docs/SPEC.md)
- [Harness research](docs/research/harnesses.md)
- [Claude Code adapter notes](docs/adapters/claude-code.md)

## Status

Claude Code works end to end: live sessions and subagents, pending approvals,
the session timeline, notifications, and Go to agent. Connect it in
Settings → Integrations, which adds observe-only hooks to
`~/.claude/settings.json` after showing you the diff. Codex, Copilot CLI, and
Cursor are detected but not followed yet.

## Repository layout

```
apps/desktop/        Tauri app: src-tauri/ (Rust shell, tray, IPC) and src/ (React UI)
crates/core/         data model, reducer, SQLite store, local ingest server (no Tauri deps)
crates/adapters/     one module per harness
crates/hook/         `agentscope-hook`, the fail-open shim harness hooks call
crates/protocol/     wire format shared by the shim and the app (kept tiny for the shim)
fixtures/            recorded hook payloads and transcript samples per harness
docs/                spec and research
```

## Development

Prerequisites: Rust (stable), Node 22, pnpm 10, and the
[Tauri system dependencies](https://v2.tauri.app/start/prerequisites/) for your OS.

```sh
cd apps/desktop
pnpm install
pnpm tauri dev          # run the app with hot reload
pnpm tauri build        # build installers into target/release/bundle
```

Both Tauri commands first run `pnpm sidecar`, which builds `agentscope-hook`
and copies it to `src-tauri/binaries/` where Tauri bundles it next to the app.
Plain `cargo` builds use an empty placeholder there instead, and the app then
reports the shim as missing.

From the repo root:

```sh
cargo test --workspace  # all Rust tests; also regenerates apps/desktop/src/bindings.ts
cargo build --release -p agentscope-hook
```

Set `AGENTSCOPE_DATA_DIR` to run the app and the shim against a scratch data
directory instead of your real one, and `CLAUDE_CONFIG_DIR` to point the Claude
Code adapter at a scratch config instead of `~/.claude`. To send a test event by hand while the app
is running:

```sh
echo '{"session_id":"demo"}' | target/release/agentscope-hook --harness claude-code --event SessionStart
```

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option. Unless you explicitly state
otherwise, any contribution you submit for inclusion in this project, as
defined in the Apache-2.0 license, is dual licensed as above, without any
additional terms or conditions.
