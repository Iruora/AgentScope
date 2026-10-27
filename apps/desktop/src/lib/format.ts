import type { AgentState, HarnessId } from "./types";

export const harnessNames: Record<HarnessId, string> = {
  "claude-code": "Claude Code",
  codex: "Codex",
  "copilot-cli": "Copilot CLI",
  cursor: "Cursor",
};

export const stateLabels: Record<AgentState, string> = {
  starting: "Starting",
  thinking: "Thinking",
  running_tool: "Running",
  waiting_approval: "Needs approval",
  maybe_waiting: "May need approval",
  waiting_input: "Waiting for you",
  idle: "Idle",
  ended: "Ended",
  errored: "Errored",
  unknown: "Unknown",
};

export function needsUser(state: AgentState): boolean {
  return state === "waiting_approval" || state === "maybe_waiting" || state === "waiting_input";
}

export function elapsed(since: string, now = Date.now()): string {
  const seconds = Math.max(0, Math.floor((now - Date.parse(since)) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

/** Last path segment, for compact repo names. */
export function basename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}
