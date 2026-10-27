import { CircleAlert, CircleDashed, CircleHelp, CircleOff, Hand, Loader, MessageCircle, Moon, Sparkles } from "lucide-react";
import type { LucideIcon } from "lucide-react";

import type { AgentState } from "../lib/types";
import { stateLabels } from "../lib/format";
import { Pill, type Tone } from "./ui";

export const stateTones: Record<AgentState, Tone> = {
  starting: "work",
  thinking: "work",
  running_tool: "work",
  waiting_approval: "warn",
  maybe_waiting: "warn",
  waiting_input: "ask",
  idle: "neutral",
  ended: "neutral",
  errored: "danger",
  unknown: "neutral",
};

const icons: Record<AgentState, LucideIcon> = {
  starting: Loader,
  thinking: Sparkles,
  running_tool: Loader,
  waiting_approval: Hand,
  maybe_waiting: Hand,
  waiting_input: MessageCircle,
  idle: Moon,
  ended: CircleOff,
  errored: CircleAlert,
  unknown: CircleHelp,
};

export function StatePill({ state }: { state: AgentState }) {
  const busy = state === "starting" || state === "running_tool";
  return (
    <Pill tone={stateTones[state]} icon={state === "maybe_waiting" ? CircleDashed : icons[state]} spin={busy} dashed={state === "maybe_waiting"}>
      {stateLabels[state]}
    </Pill>
  );
}
