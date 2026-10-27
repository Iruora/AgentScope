import { useState } from "react";

import { useGoToAgent } from "../lib/api";

/**
 * Focuses the terminal or IDE the agent runs in. When the OS does not allow
 * that, copies a `cd` into the agent's folder and says so.
 */
export function GoToAgentButton({ sessionId, primary = false }: { sessionId: string; primary?: boolean }) {
  const goTo = useGoToAgent();
  const [note, setNote] = useState<string | null>(null);

  const onClick = async (e: React.MouseEvent) => {
    e.stopPropagation();
    setNote(null);
    try {
      const result = await goTo.mutateAsync(sessionId);
      if (result.focused) return;
      let message = result.message ?? "Could not focus the agent.";
      if (result.fallbackCommand) {
        try {
          await navigator.clipboard.writeText(result.fallbackCommand);
          message += ` Copied \`${result.fallbackCommand}\`.`;
        } catch {
          message += ` Run \`${result.fallbackCommand}\`.`;
        }
      }
      setNote(message);
    } catch (err) {
      setNote(String(err instanceof Error ? err.message : err));
    }
  };

  return (
    <span className="inline-flex flex-col items-end gap-1">
      <button
        type="button"
        onClick={onClick}
        disabled={goTo.isPending}
        className={`rounded-md px-2.5 py-1 text-xs font-medium whitespace-nowrap disabled:opacity-60 ${
          primary
            ? "bg-zinc-900 text-white hover:bg-zinc-700 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-300"
            : "border border-zinc-300 hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800"
        }`}
      >
        Go to agent
      </button>
      {note && (
        <span role="status" className="max-w-64 text-right text-xs text-zinc-500">
          {note}
        </span>
      )}
    </span>
  );
}
