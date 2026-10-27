import { ChevronDown, CircleDashed, Clock, ShieldCheck, TerminalSquare } from "lucide-react";
import { useState } from "react";

import { GoToAgentButton } from "../components/GoToAgentButton";
import { Code, EmptyState, Notice, PageIntro, Pill, SectionTitle, cx } from "../components/ui";
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
    <div className="mx-auto flex max-w-[1180px] flex-col gap-5">
      <PageIntro
        title="Approvals"
        subtitle="Every permission prompt waiting on you, oldest first."
      />
      <Notice tone="accent" icon={ShieldCheck}>
        AgentScope only watches. Answer each prompt in the agent's own terminal; Go to agent takes you there.
      </Notice>

      {snapshot.isSuccess && approvals.length === 0 && (
        <EmptyState icon={ShieldCheck} title="Nothing is waiting for your approval">
          <p className="mt-1.5 text-[12px] text-muted">New permission prompts show up here the moment an agent asks.</p>
        </EmptyState>
      )}

      {approvals.length > 0 && (
        <section className="flex flex-col gap-3">
          <SectionTitle count={approvals.length} tone="warn">
            Waiting
          </SectionTitle>
          <ul className="flex flex-col gap-2.5">
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
        </section>
      )}
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
    <li className="relative flex flex-col gap-2 overflow-hidden rounded-[14px] border border-line bg-surface px-4 py-3.5 shadow-card">
      <span aria-hidden className="absolute inset-y-0 left-0 w-[3px] bg-warn" />
      <div className="flex flex-wrap items-center gap-1.5">
        <span className="text-[11px] font-semibold text-muted">{harnessNames[approval.harness]}</span>
        {where && (
          <span className="text-[11px] text-subtle" title={where}>
            {basename(where)}
          </span>
        )}
        <Pill tone="warn" icon={Clock}>
          waiting {elapsed(approval.requestedAt, now)}
        </Pill>
        {approval.confidence === "inferred" && (
          <Pill tone="warn" icon={CircleDashed} dashed>
            inferred
          </Pill>
        )}
      </div>

      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0 flex-1">
          <button
            type="button"
            onClick={() => onOpen(approval.sessionId)}
            className="max-w-full truncate text-left text-[14px] font-semibold text-ink hover:text-accent-text"
          >
            {session?.title ?? "Open session"}
          </button>
          <div className="mt-2 flex items-center gap-2 rounded-[8px] bg-surface-2 px-2.5 py-1.5">
            <TerminalSquare size={13} className="shrink-0 text-subtle" aria-hidden />
            <p className="min-w-0 truncate font-mono text-[11.5px] text-ink-2" title={approval.summary}>
              {approval.summary}
            </p>
          </div>
        </div>
        <GoToAgentButton sessionId={approval.sessionId} primary />
      </div>

      {approval.detail != null && (
        <div>
          <button
            type="button"
            aria-expanded={expanded}
            onClick={() => setExpanded((v) => !v)}
            className="inline-flex items-center gap-1 text-[11px] font-semibold text-muted hover:text-ink"
          >
            <ChevronDown size={13} className={cx("transition-transform", expanded && "rotate-180")} aria-hidden />
            {expanded ? "Hide details" : "Show details"}
            <Code className="ml-1">{approval.tool}</Code>
          </button>
          {expanded && (
            <pre className="mt-2 max-h-80 overflow-auto rounded-[10px] bg-surface-2 p-3 font-mono text-[11px] leading-relaxed text-ink-2">
              {JSON.stringify(approval.detail, null, 2)}
            </pre>
          )}
        </div>
      )}
    </li>
  );
}
