import type { Tone } from "../components/ui";
import type { AgentState, Session } from "./types";

export type SessionFilter = "all" | "approval" | "input" | "working" | "idle";

export const filterLabels: Record<Exclude<SessionFilter, "all">, string> = {
  approval: "Needs approval",
  input: "Waiting for you",
  working: "Working",
  idle: "Idle",
};

export const filterTones: Record<Exclude<SessionFilter, "all">, Tone> = {
  approval: "warn",
  input: "ask",
  working: "work",
  idle: "neutral",
};

export function filterOf(state: AgentState): Exclude<SessionFilter, "all"> {
  switch (state) {
    case "waiting_approval":
    case "maybe_waiting":
      return "approval";
    case "waiting_input":
      return "input";
    case "starting":
    case "thinking":
    case "running_tool":
      return "working";
    default:
      return "idle";
  }
}

export function liveSessions(sessions: Session[] | undefined): Session[] {
  return (sessions ?? []).filter((s) => s.endedAt === null);
}

export function countByFilter(sessions: Session[]): Record<Exclude<SessionFilter, "all">, number> {
  const counts = { approval: 0, input: 0, working: 0, idle: 0 };
  for (const s of sessions) counts[filterOf(s.state)]++;
  return counts;
}
