import type { AgentState } from "../lib/types";
import { stateLabels } from "../lib/format";

const tones: Record<AgentState, string> = {
  starting: "bg-sky-100 text-sky-800 dark:bg-sky-950 dark:text-sky-300",
  thinking: "bg-sky-100 text-sky-800 dark:bg-sky-950 dark:text-sky-300",
  running_tool: "bg-sky-100 text-sky-800 dark:bg-sky-950 dark:text-sky-300",
  waiting_approval: "bg-amber-100 text-amber-900 dark:bg-amber-950 dark:text-amber-300",
  maybe_waiting:
    "border border-dashed border-amber-400 text-amber-800 dark:border-amber-700 dark:text-amber-300",
  waiting_input: "bg-emerald-100 text-emerald-900 dark:bg-emerald-950 dark:text-emerald-300",
  idle: "bg-zinc-100 text-zinc-700 dark:bg-zinc-800 dark:text-zinc-300",
  ended: "bg-zinc-100 text-zinc-500 dark:bg-zinc-800 dark:text-zinc-400",
  errored: "bg-red-100 text-red-800 dark:bg-red-950 dark:text-red-300",
  unknown: "bg-zinc-100 text-zinc-500 dark:bg-zinc-800 dark:text-zinc-400",
};

export function StatePill({ state }: { state: AgentState }) {
  return (
    <span className={`rounded-full px-2 py-0.5 text-xs font-medium whitespace-nowrap ${tones[state]}`}>
      {stateLabels[state]}
    </span>
  );
}
