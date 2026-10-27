# Cursor adapter

Checked against the [Cursor hooks documentation](https://cursor.com/docs/agent/hooks)
(October 2026). Code: [`crates/adapters/src/cursor`](../../crates/adapters/src/cursor),
fixtures: [`fixtures/cursor/hooks-v1`](../../fixtures/cursor/hooks-v1).

## Signal sources

| Source | Where | Gives |
|---|---|---|
| Hooks (live) | `~/.cursor/hooks.json`, added by Connect | sessions, prompts, tool calls, subagents, turn ends |
| Process table | `Cursor`, `cursor-agent`, or `node …/cursor-agent/…` | liveness, the window for Go to agent |

Cursor's agent transcripts (`~/.cursor/projects/<slug>/agent-transcripts/`)
have no timestamps or ids to merge with hook events, so they are not read:
Cursor sessions show up once Cursor is connected.

## Hooks

```json
{ "version": 1, "hooks": { "stop": [
  { "command": "'/…/agentscope-hook' --harness cursor --event stop", "timeout": 5 } ] } }
```

- Cursor runs `command` through `sh` on macOS and Linux and PowerShell on
  Windows (`& "C:\…\agentscope-hook.exe" …`).
- The shim prints nothing. Cursor counts "no output" as a hook failure, and
  failures fail open unless a hook sets `failClosed`, so the action goes
  ahead with Cursor's normal approval flow. Cursor may log these as hook
  failures in its hooks output panel.
- Entries are recognized by the shim's file name in `command`; Disconnect
  removes exactly those and keeps `version` and the user's hooks.

Events registered: `sessionStart`, `sessionEnd`, `beforeSubmitPrompt`,
`preToolUse`, `postToolUse`, `postToolUseFailure`, `subagentStart`,
`subagentStop`, `stop`, `preCompact`. The `before*Execution` hooks are left
out: `preToolUse` already covers shell and MCP calls with a `tool_use_id`.

### Mapping

| Hook | Normalized | Notes |
|---|---|---|
| `sessionStart` / `sessionEnd` | `SessionStarted` / `SessionEnded` | the session is `conversation_id`; cwd is the first workspace root |
| `beforeSubmitPrompt` | `PromptSubmitted` | |
| `preToolUse` | `ToolStarted`, plus `ApprovalPossible` for `Shell` and MCP tools | dedupe key `tool_use_id` |
| `postToolUse` / `postToolUseFailure` | `ToolFinished` / `ToolFailed` | a `permission_denied` failure also resolves the approval as denied |
| `subagentStart` / `subagentStop` | `AgentStarted` / `AgentStopped` | `subagent_id` is the subagent's own conversation; its events map back onto the parent session |
| `stop` | `TurnCompleted` | |
| `preCompact` | `Compacted` | |

## Inferred approvals

Cursor has no event for "an approval prompt is showing". A shell or MCP call
is reported as `ApprovalPossible`; if neither a result nor any other event
for that agent arrives within 6 seconds, the engine turns it into an
approval with `inferred` confidence and the session shows **Maybe waiting**.
The tool result, a new prompt, or the turn end resolves it.

A command that simply runs longer than 6 seconds also shows as maybe
waiting; that is the price of having no prompt event, and why these
approvals are labeled as inferred.

## Known gaps

- The subagent map lives in memory, so a subagent that started before an app
  restart is attributed to its own conversation until it stops.
- Hook coverage in the `cursor-agent` CLI has lagged the desktop app in some
  releases; sessions there may miss events.
