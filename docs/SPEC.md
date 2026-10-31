# AgentScope v1 spec

> Every agent. Every action. One view.

Status: approved · 2026-10-26 · Research backing this spec:
[docs/research/harnesses.md](research/harnesses.md)

## 1. Goal

A cross-platform desktop app that shows every AI coding agent running on the
user's machine, the subagents each one spawned, what each is doing right now,
and which ones are blocked waiting for the user's approval.

### In scope for v1

- Harnesses installed and running locally: **Claude Code**, **Codex CLI**,
  **GitHub Copilot CLI**, **Cursor** (desktop agent; `cursor-agent` CLI best
  effort).
- macOS, Windows, Linux.
- Live view of sessions, subagent trees, current activity, pending approvals.
- Recent history (last 7 days by default) from on-disk transcripts.
- **Observe only.** Pending approvals are shown, and "Go to agent" focuses the
  terminal or IDE window where the user answers. AgentScope never answers an
  approval in v1. (v1.3 adds opt-in answering for Claude Code; see section 9.)
- Desktop notifications when an agent starts waiting on the user.

### Out of scope for v1

- Cloud agents (Copilot coding agent, Codex cloud, Claude Code on the web).
- Approve or deny from AgentScope (planned v1.x, opt-in per harness).
- Launching, stopping, or prompting agents.
- Cost dashboards beyond showing token counts the harness already reports.
- Any network traffic other than the update check. No telemetry by default.

## 2. Stack

**Tauri 2 + Rust core + React/TypeScript UI.**

- Small binaries and low idle memory matter for an always-on tray app;
  Electron would cost ~150 MB+ RAM to watch a few files.
- The heavy lifting (file watching, tailing JSONL, reading SQLite, a localhost
  ingest server, process scanning) is natural in Rust: `notify`, `rusqlite`,
  `zstd`, `axum`/`hyper`, `sysinfo`, `serde_json`.
- Tauri 2 ships tray, notifications, single-instance, updater, and signed
  installers (dmg, msi/nsis, AppImage/deb) per OS.
- UI: React + TypeScript + Vite, TanStack Query for state from the core,
  Tailwind for styling. Typed IPC via `tauri-specta` so Rust and TS types stay
  in sync.

Alternatives considered: Electron (heavier, but easier if the team prefers
pure TS), Wails (Go; smaller ecosystem for tray/updater). Tauri is the best fit
(decided 2026-10-27).

### Repo layout (step 2 scaffolds this)

```
apps/desktop/            Tauri app (src-tauri/ Rust, src/ React)
crates/core/             data model, store, event bus (no Tauri deps)
crates/adapters/         one module per harness
crates/hook/             `agentscope-hook` shim binary
docs/                    spec, research, adapter notes
fixtures/                recorded hook payloads + transcript samples per harness/version
```

## 3. Architecture

```
 harness process ──hook──▶ agentscope-hook ──▶ ┐
 (Claude/Codex/Copilot/                         │ local ingest
  Cursor)            ──http hook (Claude)────▶  │ (127.0.0.1 + token,
                                                │  or unix socket / named pipe)
 transcript files ──fs watch/tail──────────────▶│
 SQLite indexes ────periodic read-only scan────▶│
 process table ─────sysinfo scan (5s)──────────▶┘
                                                ▼
                              Adapter.normalize() → AgentEvent
                                                ▼
                         Core: reducer → in-memory graph + SQLite store
                                                ▼
                               Tauri events → React UI, tray, notifications
```

### 3.1 Signal sources, by priority

1. **Hooks (live).** Every harness has a hook system with session, tool,
   permission, and subagent events. AgentScope registers user-level hooks that
   forward the payload to the app. This is the authoritative real-time signal.
2. **Transcripts (history and backfill).** Tailed with a file watcher. They
   cover sessions started before install, fill in details hooks omit
   (assistant text, token counts), and let the app rebuild state after a
   restart.
3. **Process scan (liveness).** Detects running `claude`, `codex`, `copilot`,
   `cursor`/`cursor-agent` processes and their cwd, so a session with no recent
   hook still shows as alive, and a crashed one is marked ended.

### 3.2 The hook shim: `agentscope-hook`

A tiny static Rust binary installed next to the app and referenced from
harness hook configs as
`agentscope-hook --harness <id> --event <name>`.

- Reads the JSON payload on stdin, wraps it with `harness`, `event`,
  `received_at`, `pid`, `ppid`, and sends it to the app's ingest endpoint.
- Endpoint discovery: reads `<app-data>/ingest.json` (port + random token,
  rewritten on each app start). Unix socket on macOS/Linux, named pipe on
  Windows, localhost TCP as fallback.
- **Fail open, always:** 300 ms total budget, never writes to stdout, always
  exits 0. If the app is not running it appends to a bounded spool file
  (`<app-data>/spool/*.jsonl`, capped at 5 MB) that the app drains on start.
- Claude Code may use its native `http` hook type instead of the shim; the
  shim stays as the uniform path and for Windows reliability.

`ppid` from the shim is what lets "Go to agent" find the terminal window.

### 3.3 Hook installation

Settings → Integrations shows each detected harness with **Connect** /
**Disconnect**.

- Writes only to **user-level** config (`~/.claude/settings.json`,
  `~/.codex/hooks.json`, `~/.copilot/hooks/agentscope.json`,
  `~/.cursor/hooks.json`). Never touches project configs.
- Idempotent and reversible: our entries carry a marker
  (`"agentscope": true`, a dedicated file where the harness supports a
  hooks directory, or the shim's file name where the harness validates hook
  entries strictly, as Claude Code does). Disconnect removes exactly those
  entries.
- Backs up the original file before the first edit and shows a diff before
  writing.
- Observe mode registers only non-blocking events and returns no decision
  output, so installed hooks cannot change agent behavior.
- Codex: after writing, tell the user to run `/hooks` in Codex once to trust
  the new hook, and show "Waiting for first event" until one arrives.
- Copilot CLI also reads `.claude/settings.json` hooks; the shim's
  `--harness` flag plus payload shape (camelCase vs snake_case) dedupe this.
- Copilot CLI denies a tool call when a `preToolUse` command fails, so
  AgentScope does not register it there; tool calls come from the session's
  `events.jsonl`, which Copilot writes live.

### 3.4 Adapter interface

```rust
trait Adapter: Send + Sync {
    fn id(&self) -> HarnessId;
    /// Find the install, config dirs, version. Cheap; runs at startup and on demand.
    fn detect(&self) -> Detection;
    fn hooks_status(&self) -> HookStatus;            // NotInstalled | Installed | NeedsTrust | Outdated
    fn install_hooks(&self, plan: &HookPlan) -> Result<Diff>;
    fn uninstall_hooks(&self) -> Result<Diff>;
    /// Turn a raw hook payload into zero or more normalized events.
    fn normalize_hook(&self, raw: &RawHookEvent) -> Vec<AgentEvent>;
    /// Paths to watch for transcripts; the core owns watching and tailing.
    fn transcript_roots(&self) -> Vec<WatchRoot>;
    fn parse_transcript_chunk(&self, file: &TranscriptFile, bytes: &[u8]) -> Vec<AgentEvent>;
    /// Optional periodic scan (SQLite indexes, lock files).
    fn scan(&self, since: Timestamp) -> Vec<AgentEvent> { vec![] }
    fn match_process(&self, p: &ProcessInfo) -> Option<ProcessMatch>;
}
```

Adapters are pure translators; the core owns IO scheduling, dedupe, and
state. Each adapter ships fixture tests from real payloads per harness
version.

## 4. Data model

### 4.1 Entities

```ts
type HarnessId = "claude-code" | "codex" | "copilot-cli" | "cursor";

interface Session {
  id: string;                 // `${harness}:${nativeSessionId}`
  harness: HarnessId;
  nativeId: string;
  title?: string;             // first prompt summary or harness-provided name
  cwd: string;
  repo?: { root: string; branch?: string };
  model?: string;
  startedAt: string; lastEventAt: string; endedAt?: string;
  state: AgentState;
  pid?: number; terminal?: TerminalRef;   // for "Go to agent"
  transcriptPath?: string;
  tokens?: { input: number; output: number; cached?: number };
  rootAgentId: string;
}

interface Agent {               // the main agent or a subagent
  id: string;                   // `${sessionId}/${nativeAgentId|"main"}`
  sessionId: string;
  parentAgentId?: string;       // null for main; supports depth > 1
  kind: "main" | "subagent";
  agentType?: string;           // "Explore", "general-purpose", "explore", custom
  description?: string;         // delegated task prompt, first line
  state: AgentState;
  currentActivity?: Activity;
  startedAt: string; endedAt?: string;
  result?: "completed" | "failed" | "aborted";
}

type AgentState =
  | "starting" | "thinking" | "running_tool"
  | "waiting_approval"     // confirmed permission prompt
  | "maybe_waiting"        // inferred (Cursor), shown softer
  | "waiting_input"        // idle, agent asked the user something or finished a turn
  | "idle" | "ended" | "errored" | "unknown";

interface Activity {
  kind: "tool" | "message" | "compact";
  tool?: string; summary: string;  // "Bash: npm test", "Edit src/app.ts"
  startedAt: string;
}

interface PendingApproval {
  id: string; sessionId: string; agentId: string;
  harness: HarnessId;
  tool: string; summary: string;   // redacted, truncated
  detail?: unknown;                // full tool input, shown on expand
  requestedAt: string;
  confidence: "confirmed" | "inferred";
  resolvedAt?: string;
  resolution?: "approved" | "denied" | "cancelled" | "unknown";
}
```

### 4.2 Normalized events

Every source produces `AgentEvent`s; a single reducer applies them.

```
SessionStarted, SessionEnded, PromptSubmitted,
AgentStarted (subagent), AgentStopped,
ToolStarted, ToolFinished, ToolFailed,
ApprovalRequested, ApprovalResolved,
WaitingForInput, TurnCompleted, Compacted,
TokensReported, Heartbeat (process alive), Unknown(raw)
```

Events carry `source: hook | transcript | scan | process`, a native
timestamp, and a dedupe key (`tool_use_id` / `toolCallId` / event uuid).
Hook events win over transcript events for the same key.

### 4.3 Harness mapping (abridged)

| Normalized | Claude Code | Codex | Copilot CLI | Cursor |
|---|---|---|---|---|
| SessionStarted | `SessionStart` | `SessionStart` | `sessionStart` | `sessionStart` |
| AgentStarted | `SubagentStart` | `SubagentStart` | first event with `agentId` | `subagentStart` |
| AgentStopped | `SubagentStop` | `SubagentStop` | `subagentStop` | `subagentStop` |
| ToolStarted | `PreToolUse` | `PreToolUse` | `tool.execution_start` (events.jsonl) | `preToolUse` |
| ApprovalRequested | `PermissionRequest` | `PermissionRequest` | `notification:permission_prompt` (tool from `permissionRequest`) | inferred: `preToolUse` (shell, MCP) with no result in 6 s |
| ApprovalResolved | `PostToolUse` / `PermissionDenied` / `Stop` | `PostToolUse` / `Stop` | `tool.execution_complete` / `agentStop` | `postToolUse*` |
| WaitingForInput | `Notification:idle_prompt`, `Stop` | `Stop` | `agentStop`, `notification:elicitation_dialog` | `stop` |
| SessionEnded | `SessionEnd`, process gone | `SessionEnd` | `sessionEnd`, lock file gone | `sessionEnd` |

### 4.4 Storage

SQLite in the app data dir: `sessions`, `agents`, `approvals`, and an
`events` table (ring buffer, 7 days default, configurable). The live graph is
held in memory and rebuilt from SQLite on start. Raw tool inputs are stored
only for the retention window, and secrets-looking values (tokens, keys,
`.env` contents) are redacted before display and storage.

## 5. UI

### 5.1 Surfaces

1. **Tray / menu bar.** Icon badge with the count of agents waiting on the
   user. Menu lists waiting agents first, then running ones; clicking one opens
   it in the main window or jumps straight to the agent.
2. **Main window: Overview.** One card per live session, grouped by repo or
   by harness (toggle). A card shows harness icon, title, cwd/branch, state
   pill, current activity line, elapsed time, and a compact subagent tree
   (expand to see all). A **Needs you** strip pins sessions in
   `waiting_approval`, `maybe_waiting`, or `waiting_input` to the top.
3. **Approvals inbox.** Flat list of pending approvals across all harnesses,
   oldest first: tool, summary, repo, waiting-for timer, confidence badge.
   Action: **Go to agent**. (v1.x adds Approve / Deny here.)
4. **Session detail.** Header (harness, model, cwd, branch, tokens), the agent
   tree on the left, a timeline of normalized events on the right (prompts,
   tool calls with duration and status, approvals, subagent spawn/finish).
   Link to open the raw transcript file.
5. **History.** Ended sessions in the retention window, searchable by repo,
   harness, text.
6. **Settings.** Integrations (per-harness detect / connect / disconnect /
   status, with the config diff), notifications, retention, start at login,
   theme.
7. **First-run onboarding.** Detects installed harnesses, explains what
   connecting changes, connects with one click each, then shows live data.
   Works read-only (transcripts + process scan) even if the user connects
   nothing.

### 5.2 "Go to agent"

Best effort per OS, using `pid`/`ppid` from the shim and the process tree:

- macOS: walk up to the terminal app (Terminal, iTerm2, Ghostty, WezTerm,
  Warp, VS Code, Cursor) and activate it; iTerm2 and Terminal tabs via
  AppleScript when possible.
- Windows: find the top-level window of the ancestor process and bring it to
  front (Windows Terminal tab targeting is not possible; focus the window).
- Linux: X11 via `wmctrl`/EWMH if available; Wayland falls back to copying
  `cd <cwd>` and showing a hint.
- Cursor sessions: open the workspace via `cursor <workspace_root>`.

### 5.3 Notifications

Native notification when an agent enters `waiting_approval` (and optionally
`waiting_input`), debounced per session, clicking it runs "Go to agent".
Suppressed when the agent's terminal is already focused (best effort).

## 6. Non-functional requirements

- Idle CPU < 1%, RAM < 120 MB with 10 live sessions.
- Hook-to-UI latency < 500 ms.
- Hooks never slow an agent by more than 50 ms p95 and never fail it.
- Works with zero network access. Updates via signed Tauri updater only.
- Read-only access to all harness files except the hook config entries the
  user explicitly connects.
- Code signing and notarization on macOS, signed installers on Windows.

## 7. Delivery plan

1. **Research + spec** (this document).
2. **Stack + scaffold:** monorepo layout, Tauri app with tray and empty
   Overview, `core` crate with model + reducer + SQLite, `agentscope-hook`
   shim with ingest server, CI building all three OSes.
3. **Claude Code end to end:** detection, hook install/uninstall, transcript
   tailing with subagents, process liveness, Overview + Approvals + Session
   detail working against it, notifications, Go to agent on macOS and Windows.
4. **Codex, Copilot CLI, Cursor adapters + release:** fixtures per harness,
   Cursor inferred approvals, onboarding, History, signed installers, README,
   v1.0.0.

## 8. Decisions

Decided (2026-10-27):

- Overview groups sessions **by repo** by default (harness grouping is a toggle).
- Default history retention is **7 days**, configurable.
- The hook shim is named **`agentscope-hook`**.
- Stack is **Tauri 2 + Rust core + React/TypeScript** (no need to match
  Merge Flow).
- v1.0.0 ships **unsigned** on every platform: AgentScope is free and open
  source, and there is no free signing for macOS. The README covers the
  one-time first-launch step, and the release workflow can sign later by
  adding secrets. The in-app updater is left for a later release.

## 9. Answering approvals (v1.3)

Opt-in per harness, Claude Code only so far. Turning it on swaps the async
`PermissionRequest` hook for a synchronous one that runs
`agentscope-hook --decide` (timeout `DECIDE_MAX_WAIT_SECS` + 10 s).

1. The shim POSTs the event to `/v1/decide` and waits up to
   `DECIDE_MAX_WAIT_SECS` (130 s) for `{ "decision": null | {"behavior": "allow"} | {"behavior": "deny", "message"} }`.
2. The app ingests the event as usual, registers the request against the new
   approval id, and holds it for the decide window (Settings, 60 s default).
3. Approve or Deny (inbox, tray, popup) answers the request and marks the
   approval resolved. The shim prints Claude Code's
   `hookSpecificOutput.decision`.
4. Ask in terminal, the window running out, the approval closing elsewhere,
   or the app being unreachable all end with an empty answer: the shim prints
   nothing and Claude Code shows its own prompt. If the app is offline the
   shim spools the event like any other.

Risky-looking commands (`apps/desktop/src/lib/risky-patterns.json`, shared by
the UI and the tray) take a second click while "Confirm risky commands" is on.
The popup opens unfocused so keystrokes meant for a terminal never answer it.

Still to verify on a real machine: whether Claude Code draws its terminal
dialog while the hook waits, or only after the hook returns empty.

Phone push (also v1.3) sends desktop-notification alerts through ntfy,
Pushover, Discord/Slack webhooks or Telegram. Off by default; without
"include names and commands" the text is generic.
