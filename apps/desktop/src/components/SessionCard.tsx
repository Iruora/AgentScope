import type { Agent, Session } from "../lib/types";
import { basename, elapsed, harnessNames } from "../lib/format";
import { StatePill } from "./StatePill";

export function SessionCard({ session, agents }: { session: Session; agents: Agent[] }) {
  const main = agents.find((a) => a.id === session.rootAgentId);
  const subagents = agents.filter((a) => a.kind === "subagent" && a.state !== "ended");
  const activity = main?.currentActivity?.summary;

  return (
    <article className="rounded-xl border border-zinc-200 bg-white p-4 shadow-sm dark:border-zinc-800 dark:bg-zinc-900">
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
          {subagents.map((a) => (
            <li key={a.id} className="flex items-center justify-between gap-2 text-sm">
              <span className="truncate">{a.description ?? a.agentType ?? "Subagent"}</span>
              <StatePill state={a.state} />
            </li>
          ))}
        </ul>
      )}
    </article>
  );
}
