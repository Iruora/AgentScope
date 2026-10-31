import { ChevronRight, Clock, Cpu, FolderGit2, GitBranch, Network } from "lucide-react";

import type { Agent, Concern, Session, SessionLabel } from "../lib/types";
import { sessionTitle } from "../lib/api";
import { LabelBadges } from "./Labels";
import { ConcernLines } from "./Concerns";
import { basename, elapsed, harnessNames, needsUser } from "../lib/format";
import { GoToAgentButton } from "./GoToAgentButton";
import { StatePill, stateTones } from "./StatePill";
import { Code, Dot, Pill, cx, toneSolid } from "./ui";

/** Subagents shown before "+N more". */
const MAX_SUBAGENTS = 4;

export function SessionCard({
  session,
  agents,
  now,
  onOpen,
  concerns = [],
  label,
}: {
  label?: SessionLabel;
  session: Session;
  agents: Agent[];
  concerns?: Concern[];
  now: number;
  onOpen: (sessionId: string) => void;
}) {
  const main = agents.find((a) => a.id === session.rootAgentId);
  const subagents = agents.filter((a) => a.kind === "subagent" && a.state !== "ended");
  const busySub = subagents.find((a) => a.currentActivity);
  const activity = main?.currentActivity?.summary ?? busySub?.currentActivity?.summary;
  const attention = needsUser(session.state);
  const folder = session.repo?.root ?? session.cwd;
  const title = sessionTitle(session, label);

  return (
    <article
      role="button"
      tabIndex={0}
      aria-label={`Open ${title}`}
      onClick={() => onOpen(session.id)}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return;
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onOpen(session.id);
        }
      }}
      className="group relative flex flex-col gap-2 overflow-hidden rounded-[14px] border border-line bg-surface px-4 py-3.5 shadow-card transition-[border-color,box-shadow] hover:border-accent-line hover:shadow-[0_0_0_3px_var(--accent-ring)]"
    >
      {attention && (
        <span aria-hidden className={cx("absolute inset-y-0 left-0 w-[3px]", toneSolid[stateTones[session.state]])} />
      )}

      <div className="flex flex-wrap items-center gap-1.5">
        <span className="text-[11px] font-semibold text-muted">{harnessNames[session.harness]}</span>
        <span className="text-[11px] text-subtle" title={folder}>
          {basename(folder)}
        </span>
        <StatePill state={session.state} />
        {subagents.length > 0 && (
          <Pill tone="accent" icon={Network}>
            {subagents.length} subagent{subagents.length === 1 ? "" : "s"}
          </Pill>
        )}
      </div>

      <div className="flex min-w-0 items-center gap-2">
        <h3 className="truncate text-[14px] leading-snug font-semibold text-ink group-hover:text-accent-text" title={label?.name ? (session.title ?? undefined) : undefined}>
          {title}
        </h3>
        <LabelBadges label={label} className="shrink-0" />
      </div>

      <div className="flex min-w-0 items-center gap-1.5 text-[11px] text-subtle">
        {session.repo?.branch ? (
          <>
            <GitBranch size={13} className="shrink-0" aria-hidden />
            <Code className="max-w-64">{session.repo.branch}</Code>
          </>
        ) : (
          <>
            <FolderGit2 size={13} className="shrink-0" aria-hidden />
            <Code className="max-w-96">{session.cwd}</Code>
          </>
        )}
        {session.model && (
          <span className="ml-auto inline-flex shrink-0 items-center gap-1 text-muted">
            <Cpu size={12} aria-hidden />
            {session.model}
          </span>
        )}
      </div>

      {activity && (
        <p className="truncate rounded-[8px] bg-surface-2 px-2.5 py-1.5 font-mono text-[11.5px] text-ink-2" title={activity}>
          {activity}
        </p>
      )}

      <ConcernLines concerns={concerns} />

      {subagents.length > 0 && (
        <ul className="ml-1 flex flex-col gap-1 border-l border-line-soft pl-3">
          {subagents.slice(0, MAX_SUBAGENTS).map((a) => (
            <li key={a.id} className="flex items-center gap-2 text-[12px]">
              <Dot tone={stateTones[a.state]} />
              <span className="min-w-0 flex-1 truncate text-ink-2">{a.description ?? a.agentType ?? "Subagent"}</span>
              <StatePill state={a.state} />
            </li>
          ))}
          {subagents.length > MAX_SUBAGENTS && (
            <li className="text-[11px] text-subtle">+{subagents.length - MAX_SUBAGENTS} more</li>
          )}
        </ul>
      )}

      <div className="flex flex-wrap items-center gap-3.5 pt-0.5">
        <span className="inline-flex items-center gap-1 text-[11px] text-subtle">
          <Clock size={12} aria-hidden />
          started {elapsed(session.startedAt, now)} ago
        </span>
        <span className="ml-auto flex items-center gap-1.5">
          {attention && <GoToAgentButton sessionId={session.id} primary size="sm" />}
          <span className="grid size-7 place-items-center rounded-[8px] border border-line text-subtle transition-colors group-hover:text-accent-text">
            <ChevronRight size={14} aria-hidden />
          </span>
        </span>
      </div>
    </article>
  );
}
