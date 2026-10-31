import { Check, ChevronDown, CircleDashed, Clock, ShieldCheck, SquareTerminal, TerminalSquare, TriangleAlert, X } from "lucide-react";
import { useState } from "react";

import { GoToAgentButton } from "../components/GoToAgentButton";
import { Button, Code, EmptyState, Notice, PageIntro, Pill, SectionTitle, cx } from "../components/ui";
import { useAnswerApproval, useAnswerable, useReleaseApproval, useSettings, useSnapshot } from "../lib/api";
import { isRisky } from "../lib/risky";
import { basename, elapsed, harnessNames } from "../lib/format";
import type { PendingApproval, Session } from "../lib/types";
import { useNow } from "../lib/useNow";

/** Every pending approval across harnesses, oldest first (spec section 5.1). */
export function Approvals({ onOpen }: { onOpen: (sessionId: string) => void }) {
  const snapshot = useSnapshot();
  const now = useNow();
  const approvals = snapshot.data?.approvals ?? [];
  const sessions = new Map((snapshot.data?.sessions ?? []).map((s) => [s.id, s]));
  const answerable = new Set(useAnswerable().data ?? []);
  const settings = useSettings().data;

  return (
    <div className="mx-auto flex max-w-[1180px] flex-col gap-5">
      <PageIntro
        title="Approvals"
        subtitle="Every permission prompt waiting on you, oldest first."
      />
      <Notice tone="accent" icon={ShieldCheck}>
        {answerable.size > 0
          ? "Prompts marked Answer here wait for you in AgentScope. Anything you don't answer goes back to the agent's terminal."
          : "Answer each prompt in the agent's own terminal; Go to agent takes you there. To answer Claude Code prompts here, turn it on in Settings → Integrations."}
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
                answerable={answerable.has(a.id)}
                windowSecs={settings?.decideWindowSecs ?? 60}
                confirmRisky={settings?.confirmRisky ?? true}
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
  answerable,
  windowSecs,
  confirmRisky,
}: {
  approval: PendingApproval;
  session?: Session;
  now: number;
  onOpen: (sessionId: string) => void;
  answerable: boolean;
  windowSecs: number;
  confirmRisky: boolean;
}) {
  const [expanded, setExpanded] = useState(false);
  const left = Math.max(0, Math.ceil(windowSecs - (now - Date.parse(approval.requestedAt)) / 1000));
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
        {answerable && (
          <Pill tone="accent" icon={ShieldCheck}>
            Answer here · {left}s left
          </Pill>
        )}
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
        {answerable ? (
          <AnswerButtons approvalId={approval.id} risky={confirmRisky && isRisky(approval.summary)} />
        ) : (
          <GoToAgentButton sessionId={approval.sessionId} primary />
        )}
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

export function AnswerButtons({ approvalId, risky }: { approvalId: string; risky: boolean }) {
  const answer = useAnswerApproval();
  const release = useReleaseApproval();
  const [confirming, setConfirming] = useState(false);
  const busy = answer.isPending || release.isPending;
  const approve = () => {
    if (risky && !confirming) {
      setConfirming(true);
      return;
    }
    answer.mutate({ approvalId, allow: true });
  };

  return (
    <span className="flex flex-col items-end gap-1.5">
      <span className="flex flex-wrap justify-end gap-2">
        <Button size="sm" icon={SquareTerminal} disabled={busy} onClick={() => release.mutate(approvalId)} title="Hand this prompt back to the agent's terminal now">
          Ask in terminal
        </Button>
        <Button size="sm" icon={X} disabled={busy} onClick={() => answer.mutate({ approvalId, allow: false })}>
          Deny
        </Button>
        <Button size="sm" variant="primary" icon={confirming ? TriangleAlert : Check} disabled={busy} onClick={approve}>
          {confirming ? "Approve anyway" : "Approve"}
        </Button>
      </span>
      {confirming && <span className="text-[11px] text-warn-text">This command looks risky. Click again to approve.</span>}
      {answer.isError && <span className="max-w-[320px] text-right text-[11px] text-danger-text">{answer.error.message}</span>}
    </span>
  );
}
