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

export function needsApproval(state: AgentState): boolean {
  return state === "waiting_approval" || state === "maybe_waiting";
}

/** 1234 → "1.2k", 2_500_000 → "2.5M". */
export function compactNumber(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

export function clockTime(at: string): string {
  return new Date(at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

/** An estimated USD amount: "<$0.01", "$0.42", "$12.30", "$1,234". */
export function usd(n: number): string {
  if (n > 0 && n < 0.01) return "<$0.01";
  if (n >= 1000) return `$${Math.round(n).toLocaleString("en-US")}`;
  return `$${n.toFixed(2)}`;
}
