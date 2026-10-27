import { useState } from "react";

import { GoToAgentButton } from "../components/GoToAgentButton";
import { useSnapshot } from "../lib/api";
import { basename, elapsed, harnessNames } from "../lib/format";
import type { PendingApproval, Session } from "../lib/types";
import { useNow } from "../lib/useNow";

/** Every pending approval across harnesses, oldest first (spec section 5.1). */
export function Approvals({ onOpen }: { onOpen: (sessionId: string) => void }) {
  const snapshot = useSnapshot();
  const now = useNow();
  const approvals = snapshot.data?.approvals ?? [];
  const sessions = new Map((snapshot.data?.sessions ?? []).map((s) => [s.id, s]));

  return (
    <div className="mx-auto max-w-5xl px-6 py-6">
      <header className="mb-6">
        <h1 className="text-xl font-semibold">Approvals</h1>
        <p className="mt-1 text-sm text-zinc-500">
          AgentScope only watches. Answer each prompt in the agent's own terminal.
        </p>
      </header>

      {snapshot.isSuccess && approvals.length === 0 && (
        <div className="rounded-xl border border-dashed border-zinc-300 p-8 text-center text-sm text-zinc-500 dark:border-zinc-700">
          Nothing is waiting for your approval.
        </div>
      )}

      <ul className="space-y-3">
        {approvals.map((a) => (
          <ApprovalRow
            key={a.id}
            approval={a}
            session={sessions.get(a.sessionId)}
            now={now}
            onOpen={onOpen}
          />
        ))}
      </ul>
    </div>
  );
}

function ApprovalRow({
  approval,
  session,
  now,
  onOpen,
}: {
  approval: PendingApproval;
  session?: Session;
  now: number;
  onOpen: (sessionId: string) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const where = session ? (session.repo?.root ?? session.cwd) : "";

  return (
    <li className="rounded-xl border border-zinc-200 bg-white p-4 shadow-sm dark:border-zinc-800 dark:bg-zinc-900">
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <p className="flex flex-wrap items-center gap-1 text-xs text-zinc-500">
            <span>{harnessNames[approval.harness]}</span>
            {where && <span title={where}>· {basename(where)}</span>}
            <span>· waiting {elapsed(approval.requestedAt, now)}</span>
            {approval.confidence === "inferred" && (
              <span className="rounded-full border border-dashed border-amber-400 px-1.5 text-amber-700 dark:text-amber-300">
                inferred
              </span>
            )}
          </p>
          <p className="mt-1 truncate font-mono text-sm" title={approval.summary}>
            {approval.summary}
          </p>
          <div className="mt-2 flex gap-3 text-xs">
            <button type="button" className="text-sky-700 hover:underline dark:text-sky-400" onClick={() => onOpen(approval.sessionId)}>
              {session?.title ?? "Open session"}
            </button>
            {approval.detail != null && (
              <button
                type="button"
                className="text-zinc-500 hover:underline"
                aria-expanded={expanded}
                onClick={() => setExpanded((v) => !v)}
              >
                {expanded ? "Hide details" : "Show details"}
              </button>
            )}
          </div>
        </div>
        <GoToAgentButton sessionId={approval.sessionId} primary />
      </div>
      {expanded && (
        <pre className="mt-3 max-h-80 overflow-auto rounded-lg bg-zinc-100 p-3 text-xs dark:bg-zinc-950">
          {JSON.stringify(approval.detail, null, 2)}
        </pre>
      )}
    </li>
  );
}
