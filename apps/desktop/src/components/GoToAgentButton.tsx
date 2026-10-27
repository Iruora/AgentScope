import { SquareTerminal } from "lucide-react";
import { useState } from "react";

import { useGoToAgent } from "../lib/api";
import { Button } from "./ui";

/**
 * Focuses the terminal or IDE the agent runs in. When the OS does not allow
 * that, copies a `cd` into the agent's folder and says so.
 */
export function GoToAgentButton({
  sessionId,
  primary = false,
  size = "md",
}: {
  sessionId: string;
  primary?: boolean;
  size?: "sm" | "md";
}) {
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
      <Button
        variant={primary ? "primary" : "secondary"}
        size={size}
        icon={SquareTerminal}
        onClick={onClick}
        disabled={goTo.isPending}
      >
        Go to agent
      </Button>
      {note && (
        <span role="status" className="max-w-64 text-right text-[11px] text-muted">
          {note}
        </span>
      )}
    </span>
  );
}
