# Copilot CLI adapter

Checked against GitHub Copilot CLI 1.0.104 (October 2026), the
[hooks reference](https://docs.github.com/en/copilot/reference/hooks-reference),
and the session event schema in `github/copilot-sdk`. Code:
[`crates/adapters/src/copilot_cli`](../../crates/adapters/src/copilot_cli),
fixtures: [`fixtures/copilot-cli/1.0`](../../fixtures/copilot-cli/1.0).

## Signal sources

| Source | Where | Gives |
|---|---|---|
| Hooks (live) | `~/.copilot/hooks/agentscope.json`, written by Connect | permission prompts, questions, turn ends, subagent ends, the harness pid |
| Event log (live) | `~/.copilot/session-state/<session>/events.jsonl` | prompts, tool calls, subagent activity, model, token totals at shutdown |
| Lock files | `inuse.<pid>.lock` and `workspace.yaml` in the session folder | which sessions are running, their cwd and name |
| Process table | the `copilot` binary | liveness, the terminal for Go to agent |

`$COPILOT_HOME` replaces `~/.copilot` everywhere.

Copilot writes `events.jsonl` as things happen, so the log is a live source,
not just history. Hooks only fill in what it lacks.

## Hooks

Copilot loads every file in `~/.copilot/hooks/`, so AgentScope owns
`agentscope.json` outright: Connect writes it, Disconnect deletes it, and the
user's settings are never edited. Entries use the `exec` form, so no shell is
involved:

```json
{ "version": 1, "hooks": { "agentStop": [
  { "type": "command", "exec": "/…/agentscope-hook",
    "args": ["--harness", "copilot-cli", "--event", "agentStop"], "timeoutSec": 5 } ] } }
```

Events registered: `sessionStart`, `sessionEnd`, `permissionRequest`,
`notification`, `agentStop`, `subagentStop` (camelCase, so payloads use
camelCase fields).

`preToolUse` is deliberately **not** registered. Copilot treats a failing
`preToolUse` command as a denial, so if AgentScope were removed without
disconnecting, every tool call would be blocked. All registered events fail
open: a missing shim is logged and skipped.

### Mapping

| Hook | Normalized | Notes |
|---|---|---|
| `sessionStart` / `sessionEnd` | `SessionStarted` / `SessionEnded` | also teaches the engine the harness pid |
| `permissionRequest` | nothing yet | fires for every permission check, even ones rules approve; the tool and input are kept for the next prompt |
| `notification` `permission_prompt` | `ApprovalRequested` (confirmed) | tool and input from the preceding `permissionRequest`, or the notification text |
| `notification` `elicitation_dialog` | `WaitingForInput` | |
| `agentStop` | `TurnCompleted` | |
| `subagentStop` | `AgentStopped` | with `agentType` |

Inside a subagent, payloads carry `agentId`, `agentType` and
`parentAgentId`; the adapter uses them to hang the subagent under its parent.

## Event log

Lines are `{ "type", "data", "id", "timestamp", "parentId" }`, with
`agentId` on events from a subagent. Transient events (`session.idle`,
`assistant.usage`, deltas) are never written.

- `session.start` → `SessionStarted` (`context.cwd`, `selectedModel`);
  `session.resume` and `session.model_change` → `SessionInfo`.
- `user.message` → `PromptSubmitted`.
- `tool.execution_start` / `tool.execution_complete` → `ToolStarted` /
  `ToolFinished` or `ToolFailed`, keyed by `toolCallId`.
- An `assistant.message` with no tool requests followed by
  `assistant.turn_end` → `TurnCompleted`. A turn that ends in a tool call is
  not the end of the user's turn.
- `session.compaction_start` → `Compacted`.
- `session.shutdown` → `TokensReported` summed over `modelMetrics`, then
  `SessionEnded`.

`subagent.*` events are keyed by the spawning tool call, not by the
`agentId` the subagent's own events carry, so subagents are created from
their first event instead.

## Known gaps

- The built-in `general-purpose` agent emits no subagent events or hooks, so
  its work shows on the main agent.
- A permission prompt answered by denying it is resolved by the next tool
  result, prompt, or turn end, since Copilot sends no hook for the answer.
