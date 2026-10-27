# AgentScope

Every agent. Every action. One view.

A cross-platform desktop app that shows every local AI coding agent (Claude Code, Codex CLI, GitHub Copilot CLI, Cursor), their subagents, what they are doing, and which ones are waiting for your approval.

- [v1 spec](docs/SPEC.md)
- [Harness research](docs/research/harnesses.md)
- Adapter notes: [Claude Code](docs/adapters/claude-code.md), [Codex](docs/adapters/codex.md),
  [Copilot CLI](docs/adapters/copilot-cli.md), [Cursor](docs/adapters/cursor.md)

## Status

All four harnesses are followed: live sessions and subagents, pending
approvals, the session timeline, notifications, and Go to agent. Connect each
one in Settings → Integrations, which adds observe-only hooks after showing
you the diff:

| Harness | Hook config | Also reads |
|---|---|---|
| Claude Code | `~/.claude/settings.json` | transcripts, `~/.claude/sessions` |
| Codex | `~/.codex/hooks.json` (trust it once with `/hooks` in Codex) | rollouts in `~/.codex/sessions` |
| Copilot CLI | its own file, `~/.copilot/hooks/agentscope.json` | `events.jsonl` and lock files in `~/.copilot/session-state` |
| Cursor | `~/.cursor/hooks.json` | hooks only; approvals are inferred and shown as *maybe waiting* |

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
directory instead of your real one. `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, and
`COPILOT_HOME` point those adapters at scratch configs, as they do for the
harnesses themselves. To send a test event by hand while the app
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
