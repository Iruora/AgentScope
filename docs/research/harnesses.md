# Harness research: how local agents expose state

Research date: 2026-10-26. Harnesses change fast; every adapter must tolerate
unknown fields, missing files, and version drift. Items marked **(verify)** were
not confirmed from primary docs and must be checked against a real install in
the adapter step.

## Summary

| Harness | Hooks | Permission event | Subagent events | On-disk sessions | Feasibility |
|---|---|---|---|---|---|
| Claude Code | Yes, 30+ events, `command` and `http` handlers | `PermissionRequest`, plus `Notification` with `permission_prompt` | `SubagentStart`, `SubagentStop` | `~/.claude/projects/<encoded-cwd>/<session>.jsonl`, subagents in `<session>/subagents/` | **High** |
| Codex CLI | Yes, on by default, must be trusted via `/hooks` | `PermissionRequest` | `SubagentStart`, `SubagentStop` | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl[.zst]`, index in `~/.codex/state_*.sqlite` | **High** |
| Copilot CLI | Yes, `command` handlers | `permissionRequest`, `notification` | `subagentStart`, `subagentStop` (with `parentAgentId`) | `~/.copilot/session-state/<id>/events.jsonl` + `workspace.yaml` | **High** |
| Cursor (IDE + `cursor-agent`) | Yes, `hooks.json` | No generic one; `beforeShellExecution` / `beforeMCPExecution` can return `ask` | `subagentStart`, `subagentStop` | SQLite `state.vscdb` + `~/.cursor/projects/<slug>/agent-transcripts/` | **Medium** |

Key finding: all four harnesses now ship a hook system with near-identical
lifecycle events (session start/end, pre/post tool, permission, subagent
start/stop, stop). That makes **hooks the primary live signal** and transcript
files the secondary source for history, backfill, and sessions that started
before AgentScope was installed.

## Claude Code

- **Hook config:** `~/.claude/settings.json` (user), `.claude/settings.json` and
  `.claude/settings.local.json` (project), managed policy, plugin
  `hooks/hooks.json`. Entries merge across levels, so AgentScope can add its
  own user-level entries without touching the user's existing hooks.
- **Handler types:** `command` (stdin JSON) and `http` (POST JSON body). The
  `http` type lets hooks post straight to a localhost endpoint with no shim
  binary.
- **Common input:** `session_id`, `transcript_path`, `cwd`, `permission_mode`,
  `hook_event_name`, `prompt_id`; `agent_id` and `agent_type` inside subagents.
- **Relevant events:** `SessionStart`, `SessionEnd`, `UserPromptSubmit`,
  `PreToolUse`, `PermissionRequest`, `PermissionDenied`, `PostToolUse`,
  `PostToolUseFailure`, `Notification` (types include `permission_prompt`,
  `idle_prompt`, `agent_needs_input`, `elicitation_dialog`), `SubagentStart`,
  `SubagentStop`, `Stop`, `StopFailure`, `TaskCreated`, `TaskCompleted`,
  `CwdChanged`, `WorktreeCreate`.
- **Approvals:** `PermissionRequest` can return
  `hookSpecificOutput.decision.behavior = allow | deny`. Pending state ends on
  `PostToolUse`, `PostToolUseFailure`, `PermissionDenied`, or `Stop`.
- **Transcripts:** `~/.claude/projects/<encoded-cwd>/<session-id>.jsonl`.
  Subagents live in `<session-id>/subagents/agent-<agentId>.jsonl` with a
  sidecar `agent-<agentId>.meta.json` (`agentType`, `description`,
  `toolUseId`). Older versions inline subagent lines in the main file with
  `isSidechain: true`. Transcript writes are async and may lag hooks.
- **Liveness (verify):** no documented PID file; infer from hook activity plus
  process scan for `claude`.

## Codex CLI

- **Hook config:** `~/.codex/hooks.json` or `[hooks]` in `~/.codex/config.toml`;
  project `.codex/` only when the project is trusted. Hooks are on by default
  (`[features] hooks`), but non-managed hooks must be **trusted by the user via
  `/hooks`** before they run. Onboarding must explain this step.
- **Common input:** `session_id`, `transcript_path`, `cwd`, `hook_event_name`,
  `model`; `turn_id` on turn-scoped events.
- **Events:** `SessionStart`, `SessionEnd`, `UserPromptSubmit`, `PreToolUse`,
  `PermissionRequest`, `PostToolUse`, `SubagentStart`, `SubagentStop`, `Stop`,
  `Interrupt`, `PreCompact`, `PostCompact`. `PermissionRequest` only fires for
  calls that actually need approval, which is exactly the signal we want.
- **Windows:** supports `commandWindows` overrides.
- **Transcripts:** `~/.codex/sessions/YYYY/MM/DD/rollout-<uuid>.jsonl`, newer
  builds zstd-compressed (`.jsonl.zst`). Records: `session_meta`,
  `response_item`, `event_msg`, approval decisions. Thread index in
  `~/.codex/state_<n>.sqlite` (schema version in the filename; read-only, open
  with `immutable`/`mode=ro`).
- **App server:** `codex app-server` speaks JSON-RPC (stdio, experimental
  WebSocket) with thread/turn/item primitives and approval requests such as
  `item/commandExecution/requestApproval`. Useful for sessions AgentScope
  launches itself (post-v1), not for attaching to a user's running TUI.

## GitHub Copilot CLI

- **Hook config:** user `~/.copilot/hooks/*.json` (or `$COPILOT_HOME/hooks/`)
  and `hooks` in `~/.copilot/settings.json`; repo `.github/hooks/*.json`; policy
  dirs. It also reads `.claude/settings.json` hooks, so AgentScope must dedupe
  when both harnesses report (tag the source via the hook's own config).
- **Events:** `sessionStart`, `sessionEnd`, `userPromptSubmitted`,
  `preToolUse`, `postToolUse`, `postToolUseFailure`, `permissionRequest`,
  `notification` (async, includes permission prompts), `agentStop`,
  `subagentStart`, `subagentStop`, `errorOccurred`, `preCompact`. camelCase
  names use camelCase payload fields; PascalCase aliases use snake_case.
- **Subagents:** inputs inside a subagent carry `agentId`, `agentType`,
  `parentAgentId`, so nesting depth > 1 is representable.
- **Session state:** `~/.copilot/session-state/<session-id>/events.jsonl`
  (append-only; types like `session.start`, `session.shutdown`,
  `tool.execution_start`, `tool.execution_complete`, `subagent.started`,
  `subagent.completed`), `workspace.yaml` (cwd, summary, name), and an
  `inuse.<PID>.lock` file that gives cheap liveness.

## Cursor

- **Hook config:** `~/.cursor/hooks.json` (user), `<project>/.cursor/hooks.json`,
  enterprise paths per OS. Applies to the desktop agent; CLI coverage is
  partial **(verify `cursor-agent`)**.
- **Common input:** `conversation_id`, `generation_id`, `transcript_path`,
  `workspace_roots`, `model`, `cursor_version`.
- **Events:** `sessionStart`, `sessionEnd`, `preToolUse`, `postToolUse`,
  `beforeShellExecution`, `afterShellExecution`, `beforeMCPExecution`,
  `afterMCPExecution`, `beforeReadFile`, `afterFileEdit`, `beforeSubmitPrompt`,
  `subagentStart`, `subagentStop`, `stop`, `afterAgentResponse`.
- **Approvals:** there is no "a prompt is showing" event. We can only infer:
  a `beforeShellExecution`/`beforeMCPExecution` not followed by the matching
  `after*` within a short window, while Cursor is not in auto-run mode, is
  "probably awaiting approval". Shown in the UI as *likely waiting* rather than
  a confirmed pending approval.
- **Storage:** `state.vscdb` (SQLite) in the Cursor user dir
  (`~/Library/Application Support/Cursor/User/globalStorage/` on macOS,
  `%APPDATA%\Cursor\User\globalStorage\` on Windows,
  `~/.config/Cursor/User/globalStorage/` on Linux **(verify)**), keys
  `composer.composerHeaders`, `composerData:<id>`, `bubbleId:<c>:<b>`. The DB
  can be many GB and is locked by the running app: read-only, never on the UI
  thread, history only. Agent transcripts JSONL under
  `~/.cursor/projects/<slug>/agent-transcripts/` are lighter and preferred.

## Cross-cutting risks

1. **Formats are undocumented and change.** Transcript parsing is best-effort;
   hooks are the contract. Adapters version-gate parsers and log unknowns.
2. **Hooks require config edits.** Installing must be explicit, reversible,
   idempotent, and leave a marker so uninstall removes only our entries.
3. **Hooks must never break the agent.** Our handlers fail open: short
   timeout, exit 0, no output that changes behavior (in observe mode).
4. **Codex hook trust** needs a user action in the Codex TUI.
5. **Duplicate signals** (Copilot reading Claude hooks; IDE plus CLI for
   Cursor) need source tagging and dedupe.
6. **Privacy:** transcripts contain code and secrets. Everything stays local;
   no telemetry by default.
