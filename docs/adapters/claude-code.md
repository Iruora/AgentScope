# Claude Code adapter

Checked against Claude Code 2.1.311 (October 2026). Code:
[`crates/adapters/src/claude_code`](../../crates/adapters/src/claude_code),
fixtures: [`fixtures/claude-code/2.1`](../../fixtures/claude-code/2.1).

## Signal sources

| Source | Where | Gives |
|---|---|---|
| Hooks (live) | `~/.claude/settings.json`, added by Connect | sessions, prompts, tool calls, permission prompts, subagents, turn ends |
| Transcripts | `~/.claude/projects/<encoded-cwd>/<session>.jsonl` and `<session>/subagents/agent-<id>.jsonl` + `.meta.json` | history, sessions started before Connect, token usage, model, title |
| Live sessions | `~/.claude/sessions/<pid>.json` (`pid`, `sessionId`, `cwd`, `status`) | which sessions have a running process, even without hooks |
| Process table | `claude` binary, or `node`/`bun` running the npm CLI | liveness, the terminal window for Go to agent |

`$CLAUDE_CONFIG_DIR` replaces `~/.claude` everywhere, as it does for Claude Code.

## Hooks

Connect adds one matcher group per event to the user settings file. Each runs
the bundled shim in exec form, so no shell is involved and paths with spaces
work on every OS:

```json
{ "hooks": [{ "type": "command", "command": "/Applications/AgentScope.app/Contents/MacOS/agentscope-hook",
              "args": ["--harness", "claude-code", "--event", "PreToolUse"],
              "async": true, "timeout": 10 }] }
```

- `async: true` makes Claude Code run the hook in the background, so it never
  waits on AgentScope. The shim also never prints and always exits 0, so no
  hook can approve, deny, or block anything (observe only, spec 3.3).
- Our entries are recognized by the command's file name (`agentscope-hook`,
  `.exe` on Windows) rather than an extra `"agentscope": true` key, because
  Claude Code validates hook entries and an unknown key is a risk on some
  versions. Disconnect removes exactly those entries and drops groups or
  events left empty. Every other key, hook, and the key order are kept.
- The first edit backs up the file to `settings.json.agentscope-backup`.
- Status is `installed` when every event has exactly the current entry,
  `outdated` when some entries point at an old shim path (the app moved or
  was updated), and `not_installed` otherwise.

Events registered: `SessionStart`, `SessionEnd`, `UserPromptSubmit`,
`PreToolUse`, `PermissionRequest`, `PermissionDenied`, `PostToolUse`,
`PostToolUseFailure`, `Notification`, `SubagentStart`, `SubagentStop`, `Stop`,
`StopFailure`, `PreCompact`.

### Mapping

| Hook | Normalized | Notes |
|---|---|---|
| `SessionStart` | `SessionStarted` | `model`, `transcript_path`, `cwd` |
| `SessionEnd` | `SessionEnded` | |
| `UserPromptSubmit` | `PromptSubmitted` | dedupe key `prompt_id`, which transcripts carry as `promptId` |
| `PreToolUse` | `ToolStarted` | dedupe key `tool_use_id`; `AskUserQuestion` also sets `WaitingForInput` |
| `PermissionRequest` | `ApprovalRequested` (confirmed) | no `tool_use_id` in the payload; resolved by the next tool result, denial, prompt, or turn end |
| `PermissionDenied` | `ApprovalResolved` (denied) | |
| `PostToolUse` / `PostToolUseFailure` | `ToolFinished` / `ToolFailed` | resolve the agent's open approval |
| `Notification` | `WaitingForInput` | `idle_prompt`, `agent_needs_input`, `elicitation_dialog`; `permission_prompt` is skipped because `PermissionRequest` already reported it |
| `SubagentStart` / `SubagentStop` | `AgentStarted` / `AgentStopped` | `agent_id` is the subagent; events from inside it carry the same `agent_id` |
| `Stop`, `StopFailure` | `TurnCompleted` | |
| `PreCompact` | `Compacted` | |

Payloads without `session_id` and `hook_event_name` are ignored: Copilot CLI
can run Claude-style hooks but sends camelCase payloads.

The shim's parent is Claude Code itself with the exec form; the engine still
walks up the process tree to the nearest process the adapter recognizes, so a
shell in between does not matter.

## Transcripts

Hooks win over transcripts for the same dedupe key (spec 4.2). Transcript
lines map as follows:

- `user` with text content → `PromptSubmitted`, except `isMeta` lines and
  text Claude Code injects (`<command-name>…`, `[Request interrupted…`).
- `user` with `tool_result` → `ToolFinished` or `ToolFailed` (`is_error`).
- `assistant` → `ToolStarted` per `tool_use`, `TokensUsed` per message (a
  message spans several lines with the same `message.id`, deduped), and
  `TurnCompleted` on `stop_reason: end_turn`. In a subagent file `end_turn`
  is `AgentStopped`.
- `custom-title` (from `/rename`) and `ai-title` set the session title.
- The first chunk of a main file emits `SessionStarted` with the transcript
  path; of a subagent file, `AgentStarted` with `agentType` and
  `description` from the sidecar `.meta.json`.
- Older versions wrote subagents inline with `isSidechain: true`; those lines
  are attributed by `agentId`, or skipped when it is missing.

Read offsets are stored, so a restart does not replay old lines. On first run
files written in the last 7 days are read; older ones are only followed from
their current end.

## Liveness

Every 5 seconds the engine re-reads `sessions/*.json` and the process table. A
live session ends when its pid is gone (or reused by another program), or,
when no pid is known, after 10 minutes without events and no Claude Code
process in its folder. New activity on an ended session reopens it, as with
`claude --resume`.

## Known gaps

- `sessions/<pid>.json` also has `status: busy | idle | waiting` and
  `waitingFor`. It is not used yet; it could show waiting sessions when hooks
  are not connected.
- Permission prompts answered in a way that fires no hook (for example an
  interrupt) stay pending until the next prompt or turn end.
