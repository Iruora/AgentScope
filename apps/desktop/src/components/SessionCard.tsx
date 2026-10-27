import type { Agent, Session } from "../lib/types";
import { basename, elapsed, harnessNames, needsUser } from "../lib/format";
import { GoToAgentButton } from "./GoToAgentButton";
import { StatePill } from "./StatePill";

/** Subagents shown before "+N more". */
const MAX_SUBAGENTS = 4;

export function SessionCard({
  session,
  agents,
  onOpen,
}: {
  session: Session;
  agents: Agent[];
  onOpen: (sessionId: string) => void;
}) {
  const main = agents.find((a) => a.id === session.rootAgentId);
  const subagents = agents.filter((a) => a.kind === "subagent" && a.state !== "ended");
  const busySub = subagents.find((a) => a.currentActivity);
  const activity = main?.currentActivity?.summary ?? busySub?.currentActivity?.summary;

  return (
    <article
      role="button"
      tabIndex={0}
      onClick={() => onOpen(session.id)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onOpen(session.id);
        }
      }}
      className="cursor-pointer rounded-xl border border-zinc-200 bg-white p-4 shadow-sm transition hover:border-zinc-400 focus-visible:outline-2 focus-visible:outline-sky-500 dark:border-zinc-800 dark:bg-zinc-900 dark:hover:border-zinc-600"
    >
      <header className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <p className="text-xs text-zinc-500">{harnessNames[session.harness]}</p>
          <h3 className="truncate font-medium">{session.title ?? "Untitled session"}</h3>
        </div>
        <StatePill state={session.state} />
      </header>
      <p className="mt-1 truncate text-xs text-zinc-500" title={session.cwd}>
        {basename(session.cwd)}
        {session.repo?.branch ? ` · ${session.repo.branch}` : ""}
        {` · ${elapsed(session.startedAt)}`}
      </p>
      {activity && <p className="mt-3 truncate font-mono text-sm">{activity}</p>}
      {subagents.length > 0 && (
        <ul className="mt-3 space-y-1 border-l border-zinc-200 pl-3 dark:border-zinc-700">
          {subagents.slice(0, MAX_SUBAGENTS).map((a) => (
            <li key={a.id} className="flex items-center justify-between gap-2 text-sm">
              <span className="truncate">{a.description ?? a.agentType ?? "Subagent"}</span>
              <StatePill state={a.state} />
            </li>
          ))}
          {subagents.length > MAX_SUBAGENTS && (
            <li className="text-xs text-zinc-500">+{subagents.length - MAX_SUBAGENTS} more</li>
          )}
        </ul>
      )}
      {needsUser(session.state) && (
        <footer className="mt-3 flex justify-end">
          <GoToAgentButton sessionId={session.id} primary />
        </footer>
      )}
    </article>
  );
}
