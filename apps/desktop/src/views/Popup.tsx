import { ExternalLink, ShieldCheck, TerminalSquare, X } from "lucide-react";

import { IconButton, Pill } from "../components/ui";
import { closePopup, showSession, useAnswerable, useGraphUpdates, useSettings, useSnapshot } from "../lib/api";
import { basename, harnessNames } from "../lib/format";
import { isRisky } from "../lib/risky";
import { useNow } from "../lib/useNow";
import { AnswerButtons } from "./Approvals";

/**
 * The small always-on-top window for prompts that can be answered here.
 * The app closes it once nothing is left to answer.
 */
export function Popup() {
  useGraphUpdates();
  const snapshot = useSnapshot();
  const answerable = new Set(useAnswerable().data ?? []);
  const settings = useSettings().data;
  const now = useNow();
  const sessions = new Map((snapshot.data?.sessions ?? []).map((s) => [s.id, s]));
  const approvals = (snapshot.data?.approvals ?? []).filter((a) => answerable.has(a.id));
  const windowSecs = settings?.decideWindowSecs ?? 60;

  return (
    <div className="flex h-screen flex-col bg-bg text-ink">
      <header className="flex items-center gap-2 border-b border-line-soft px-4 py-2.5">
        <ShieldCheck size={15} className="text-accent-text" aria-hidden />
        <h1 className="flex-1 text-[13px] font-bold">
          {approvals.length === 1 ? "An agent needs your approval" : `${approvals.length} agents need your approval`}
        </h1>
        <IconButton icon={X} label="Close until the next prompt" onClick={() => void closePopup()} />
      </header>
      <ul className="flex flex-1 flex-col gap-2.5 overflow-y-auto p-3">
        {approvals.map((a) => {
          const session = sessions.get(a.sessionId);
          const where = session ? (session.repo?.root ?? session.cwd) : "";
          const left = Math.max(0, Math.ceil(windowSecs - (now - Date.parse(a.requestedAt)) / 1000));
          return (
            <li key={a.id} className="flex flex-col gap-2 rounded-[12px] border border-line bg-surface px-3 py-2.5 shadow-card">
              <div className="flex items-center gap-1.5">
                <span className="text-[11px] font-semibold text-muted">{harnessNames[a.harness]}</span>
                {where && (
                  <span className="min-w-0 truncate text-[11px] text-subtle" title={where}>
                    {basename(where)}
                  </span>
                )}
                <button
                  type="button"
                  onClick={() => void showSession(a.sessionId)}
                  className="inline-flex items-center gap-1 text-[11px] font-semibold text-accent-text hover:underline"
                >
                  Details <ExternalLink size={11} aria-hidden />
                </button>
                <span className="flex-1" />
                <Pill tone="accent">{left}s left</Pill>
              </div>
              <div className="flex items-center gap-2 rounded-[8px] bg-surface-2 px-2.5 py-1.5">
                <TerminalSquare size={13} className="shrink-0 text-subtle" aria-hidden />
                <p className="min-w-0 truncate font-mono text-[11.5px] text-ink-2" title={a.summary}>
                  {a.summary}
                </p>
              </div>
              <AnswerButtons approvalId={a.id} risky={(settings?.confirmRisky ?? true) && isRisky(a.summary)} />
            </li>
          );
        })}
      </ul>
    </div>
  );
}
