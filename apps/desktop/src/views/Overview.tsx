import { useMemo, useState } from "react";

import type { Agent, AppInfo, Session } from "../lib/types";
import { SessionCard } from "../components/SessionCard";
import { useAppInfo, useSnapshot } from "../lib/api";
import { harnessNames, needsUser } from "../lib/format";

type GroupBy = "repo" | "harness";

const GROUP_BY_KEY = "overview.groupBy";

function loadGroupBy(): GroupBy {
  try {
    return localStorage.getItem(GROUP_BY_KEY) === "harness" ? "harness" : "repo";
  } catch {
    return "repo";
  }
}

function groupKey(session: Session, by: GroupBy): string {
  if (by === "harness") return harnessNames[session.harness];
  return session.repo?.root || session.cwd || "Unknown location";
}

export function Overview({ onOpen }: { onOpen: (sessionId: string) => void }) {
  const snapshot = useSnapshot();
  const [groupBy, setGroupBy] = useState<GroupBy>(loadGroupBy);

  const chooseGroupBy = (by: GroupBy) => {
    setGroupBy(by);
    try {
      localStorage.setItem(GROUP_BY_KEY, by);
    } catch {
      // Grouping just won't be remembered.
    }
  };

  const live = useMemo(
    () => (snapshot.data?.sessions ?? []).filter((s) => s.endedAt === null),
    [snapshot.data],
  );
  const agentsBySession = useMemo(() => {
    const map = new Map<string, Agent[]>();
    for (const agent of snapshot.data?.agents ?? []) {
      map.set(agent.sessionId, [...(map.get(agent.sessionId) ?? []), agent]);
    }
    return map;
  }, [snapshot.data]);

  const needsYou = live.filter((s) => needsUser(s.state));
  const groups = useMemo(() => {
    const map = new Map<string, Session[]>();
    for (const s of live.filter((s) => !needsUser(s.state))) {
      const key = groupKey(s, groupBy);
      map.set(key, [...(map.get(key) ?? []), s]);
    }
    return [...map.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [live, groupBy]);

  const card = (s: Session) => (
    <SessionCard key={s.id} session={s} agents={agentsBySession.get(s.id) ?? []} onOpen={onOpen} />
  );

  return (
    <div className="mx-auto max-w-5xl px-6 py-6">
      <header className="mb-6 flex items-center justify-between gap-4">
        <h1 className="text-xl font-semibold">Overview</h1>
        <div
          role="group"
          aria-label="Group sessions by"
          className="flex rounded-lg border border-zinc-200 p-0.5 text-sm dark:border-zinc-800"
        >
          {(["repo", "harness"] as const).map((by) => (
            <button
              key={by}
              type="button"
              aria-pressed={groupBy === by}
              onClick={() => chooseGroupBy(by)}
              className={`rounded-md px-3 py-1 ${
                groupBy === by
                  ? "bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900"
                  : "text-zinc-600 dark:text-zinc-400"
              }`}
            >
              By {by}
            </button>
          ))}
        </div>
      </header>

      {snapshot.isError && (
        <p className="rounded-lg bg-red-50 p-4 text-sm text-red-800 dark:bg-red-950 dark:text-red-300">
          Could not load agents: {String(snapshot.error)}
        </p>
      )}

      {needsYou.length > 0 && (
        <section className="mb-8">
          <h2 className="mb-3 text-sm font-semibold text-amber-700 dark:text-amber-400">
            Needs you · {needsYou.length}
          </h2>
          <div className="grid gap-3 sm:grid-cols-2">{needsYou.map(card)}</div>
        </section>
      )}

      {groups.map(([name, sessions]) => (
        <section key={name} className="mb-8">
          <h2 className="mb-3 truncate text-sm font-semibold text-zinc-500" title={name}>
            {name}
          </h2>
          <div className="grid gap-3 sm:grid-cols-2">{sessions.map(card)}</div>
        </section>
      ))}

      {snapshot.isSuccess && live.length === 0 && <EmptyState />}
    </div>
  );
}

function EmptyState() {
  const info = useAppInfo();
  return (
    <div className="rounded-xl border border-dashed border-zinc-300 p-8 text-center dark:border-zinc-700">
      <h2 className="font-medium">No agents running</h2>
      <p className="mx-auto mt-2 max-w-md text-sm text-zinc-500">
        Sessions from Claude Code, Codex, Copilot CLI, and Cursor will show up here as soon as they
        start. Connect a harness in Settings to see approvals live.
      </p>
      {info.data && <Harnesses info={info.data} />}
    </div>
  );
}

function Harnesses({ info }: { info: AppInfo }) {
  return (
    <ul className="mx-auto mt-6 grid max-w-md grid-cols-2 gap-2 text-left text-sm">
      {info.harnesses.map((h) => (
        <li
          key={h.harness}
          className="flex items-center justify-between rounded-lg bg-zinc-100 px-3 py-2 dark:bg-zinc-900"
        >
          <span>{harnessNames[h.harness]}</span>
          <span className={h.installed ? "text-emerald-600 dark:text-emerald-400" : "text-zinc-400"}>
            {h.installed ? "Detected" : "Not found"}
          </span>
        </li>
      ))}
    </ul>
  );
}
