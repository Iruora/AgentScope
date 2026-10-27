# Codex adapter

Checked against the Codex CLI source at 0.165 (October 2026, `openai/codex`
`codex-rs/hooks` and `codex-rs/rollout`). Code:
[`crates/adapters/src/codex`](../../crates/adapters/src/codex), fixtures:
[`fixtures/codex/0.165`](../../fixtures/codex/0.165).

## Signal sources

| Source | Where | Gives |
|---|---|---|
| Hooks (live) | `~/.codex/hooks.json`, added by Connect | sessions, prompts, tool calls, permission prompts, subagents, turn ends |
| Rollouts | `~/.codex/sessions/YYYY/MM/DD/rollout-<time>-<thread>.jsonl` | history, sessions started before Connect, token totals, model, subagent role and nickname |
| Process table | the `codex` binary | liveness, the terminal for Go to agent |

`$CODEX_HOME` replaces `~/.codex` everywhere, as it does for Codex.

## Hooks

Codex reads `hooks.json` with the same shape as Claude Code's `hooks`
setting. Connect adds one matcher group per event:

```json
{ "hooks": [{ "type": "command", "async": true, "timeout": 3,
              "command": "'/Applications/AgentScope.app/Contents/MacOS/agentscope-hook' --harness codex --event Stop" }] }
```

- Codex runs `command` through a shell (`sh -lc`, or `cmd /C` on Windows),
  so the path is quoted for that shell.
- `async: true` runs the hook in the background. Codex always runs
  `SessionEnd` synchronously and warns about `async` there, so that entry
  leaves it out. `SessionEnd` is capped at 3 seconds; the shim gives up after
  300 ms, so every entry uses 3.
- Our entries are recognized by the shim's file name inside `command`;
  Disconnect removes exactly those and keeps the user's own hooks. The first
  edit backs up `hooks.json.agentscope-backup`.

### Trust

Codex skips a user hook until it is trusted in the `/hooks` view, which
stores the SHA-256 of the hook's normalized definition in `config.toml`:

```toml
[hooks.state."/home/me/.codex/hooks.json:stop:3:0"]
trusted_hash = "sha256:…"
```

The status is `needs_trust` until every one of our entries has a matching
hash, and Settings tells the user to run `/hooks` in Codex. The hash is the
SHA-256 of the compact, key-sorted JSON of
`{ "event_name": "<snake_case event>", "hooks": [<our entry>] }`, mirroring
`hook_hash` in `codex-rs/hooks/src/engine/discovery.rs`. If a Codex update
changes that recipe, the status stays `needs_trust` even though events
arrive; the hooks themselves keep working.

Events registered: `SessionStart`, `SessionEnd`, `UserPromptSubmit`,
`PreToolUse`, `PermissionRequest`, `PostToolUse`, `SubagentStart`,
`SubagentStop`, `Stop`, `Interrupt`, `PreCompact`.

### Mapping

| Hook | Normalized | Notes |
|---|---|---|
| `SessionStart` | `SessionStarted` | `model`, `cwd`, `transcript_path` |
| `SessionEnd` | `SessionEnded` | |
| `UserPromptSubmit` | `PromptSubmitted` | dedupe key `turn_id`, shared with the rollout |
| `PreToolUse` / `PostToolUse` | `ToolStarted` / `ToolFinished` | dedupe key `tool_use_id`, the rollout's `call_id` |
| `PermissionRequest` | `ApprovalRequested` (confirmed) | fires only when the call needs the user |
| `SubagentStart` / `SubagentStop` | `AgentStarted` / `AgentStopped` | `agent_id` is the subagent's thread id; `session_id` stays the root session |
| `Stop`, `Interrupt` | `TurnCompleted` | |
| `PreCompact` | `Compacted` | |

## Rollouts

Every thread, including each subagent, writes its own rollout. The first line
is `session_meta`: `id` is the thread, `session_id` the root session, and
`parent_thread_id`, `agent_role` and `agent_nickname` describe a subagent.
Later chunks re-read that first line to know whose file they are.

- `turn_context` → `SessionInfo` with `cwd` and `model`.
- `event_msg` `user_message` → `PromptSubmitted`, except injected context
  (`<environment_context>…`).
- `response_item` `function_call`, `custom_tool_call` (`apply_patch`),
  `local_shell_call` → `ToolStarted`; `*_output` → `ToolFinished`.
- `event_msg` `task_complete` → `TurnCompleted` (`AgentStopped` in a
  subagent's rollout); `turn_aborted` likewise.
- `event_msg` `token_count` → `TokensReported` from `total_token_usage`;
  cached input is reported separately.

Codex recompresses cold rollouts to `.jsonl.zst`; those are not read, which
only matters for sessions older than AgentScope's first run.

## Known gaps

- Nested subagents attach to the main agent: hooks do not name the parent,
  and rollouts only do when they are read.
- Codex can be told to bypass hook trust or disable hooks entirely
  (`[features] hooks = false`); AgentScope then sees rollouts only.
