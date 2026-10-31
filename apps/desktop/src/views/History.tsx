import { ChevronRight, CircleAlert, Clock, Coins, FolderGit2, History as HistoryIcon, Network, Search, X } from "lucide-react";
import { useMemo, useState } from "react";

import { Card, Code, EmptyState, Label, Notice, PageIntro, cx } from "../components/ui";
import { sessionTitle, useCosts, useLabels, useSettings, useSnapshot } from "../lib/api";
import { LabelBadges, LabelChips, labelText, matchesLabel, pinnedFirst, type LabelFilter } from "../components/Labels";
import { basename, compactNumber, elapsed, harnessNames, usd } from "../lib/format";
import type { HarnessId, Session, SessionLabel } from "../lib/types";

function matchesSearch(session: Session, query: string, label?: SessionLabel): boolean {
  if (!query) return true;
  const haystack = [session.title, session.cwd, session.repo?.root, session.repo?.branch, session.model, labelText(label)]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
  return haystack.includes(query.toLowerCase());
}

/** "Today", "Yesterday", or a date, for grouping by the day a session ended. */
function dayLabel(at: string, now: Date): string {
  const day = new Date(at);
  const startOf = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const days = Math.round((startOf(now) - startOf(day)) / 86_400_000);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  return day.toLocaleDateString([], { weekday: "long", month: "short", day: "numeric" });
}

export function History({ onOpen }: { onOpen: (sessionId: string) => void }) {
  const snapshot = useSnapshot();
  const settings = useSettings();
  const [query, setQuery] = useState("");
  const [harness, setHarness] = useState<HarnessId | null>(null);
  const [labelFilter, setLabelFilter] = useState<LabelFilter>(null);
  const labels = useLabels().data;

  const ended = useMemo(
    () =>
      (snapshot.data?.sessions ?? [])
        .filter((s): s is Session & { endedAt: string } => s.endedAt != null)
        .sort((a, b) => Date.parse(b.endedAt) - Date.parse(a.endedAt)),
    [snapshot.data],
  );
  const subagentCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const a of snapshot.data?.agents ?? []) {
      if (a.kind === "subagent") counts.set(a.sessionId, (counts.get(a.sessionId) ?? 0) + 1);
    }
    return counts;
  }, [snapshot.data]);

  const harnesses = [...new Set(ended.map((s) => s.harness))];
  const visible = ended.filter(
    (s) =>
      (harness === null || s.harness === harness) &&
      matchesLabel(labelFilter, labels?.get(s.id)) &&
      matchesSearch(s, query, labels?.get(s.id)),
  );
  const now = new Date();
  const days = new Map<string, typeof visible>();
  // Pinned sessions get their own group on top; they may be older than retention.
  const pinned = pinnedFirst(visible, labels).filter((s) => labels?.get(s.id)?.pinned);
  if (pinned.length > 0) days.set("Pinned", pinned);
  for (const s of visible.filter((s) => !labels?.get(s.id)?.pinned)) {
    const label = dayLabel(s.endedAt, now);
    days.set(label, [...(days.get(label) ?? []), s]);
  }
  const retention = settings.data?.retentionDays;

  return (
    <div className="mx-auto flex max-w-[1180px] flex-col gap-5">
      <PageIntro
        title="History"
        subtitle={
          retention
            ? `Sessions that ended in the last ${retention === 1 ? "day" : `${retention} days`}. Change how long they are kept in Settings.`
            : "Sessions that ended recently."
        }
      />

      {snapshot.isError && (
        <Notice tone="danger" icon={CircleAlert}>
          Could not load history: {String(snapshot.error)}
        </Notice>
      )}

      {ended.length > 0 && (
        <div className="flex flex-wrap items-center gap-2">
          <label className="flex h-8 w-[280px] items-center gap-2 rounded-[9px] border border-line bg-surface px-2.5 focus-within:border-accent focus-within:shadow-[0_0_0_3px_var(--accent-ring)]">
            <Search size={14} className="shrink-0 text-subtle" aria-hidden />
            <input
              type="search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search title, repo, branch or tag"
              aria-label="Search history"
              className="min-w-0 flex-1 bg-transparent text-[12px] outline-none placeholder:text-subtle [&::-webkit-search-cancel-button]:hidden"
            />
            {query && (
              <button type="button" aria-label="Clear search" onClick={() => setQuery("")} className="text-subtle hover:text-ink">
                <X size={13} aria-hidden />
              </button>
            )}
          </label>
          {harnesses.length > 1 &&
            [null, ...harnesses].map((h) => (
              <button
                key={h ?? "all"}
                type="button"
                aria-pressed={harness === h}
                onClick={() => setHarness(h)}
                className={cx(
                  "inline-flex h-7 items-center rounded-full border px-2.5 text-[11px] font-semibold transition-colors",
                  harness === h
                    ? "border-accent-line bg-accent-soft text-accent-text"
                    : "border-line bg-surface text-muted hover:bg-surface-2",
                )}
              >
                {h ? harnessNames[h] : "All harnesses"}
              </button>
            ))}
          <LabelChips ids={ended.map((s) => s.id)} labels={labels} value={labelFilter} onChange={setLabelFilter} />
        </div>
      )}

      {[...days.entries()].map(([label, sessions]) => (
        <section key={label} className="flex flex-col gap-2">
          <Label>
            {label} · {sessions.length}
          </Label>
          <Card className="overflow-hidden">
            <ul className="divide-y divide-line-soft">
              {sessions.map((s) => (
                <HistoryRow key={s.id} session={s} label={labels?.get(s.id)} subagents={subagentCounts.get(s.id) ?? 0} onOpen={onOpen} />
              ))}
            </ul>
          </Card>
        </section>
      ))}

      {ended.length > 0 && visible.length === 0 && (
        <Card className="px-6 py-10 text-center text-[12px] text-muted">No sessions match this search.</Card>
      )}

      {snapshot.isSuccess && ended.length === 0 && (
        <EmptyState icon={HistoryIcon} title="Nothing here yet">
          <p className="mt-1.5 max-w-md text-[12px] leading-relaxed text-muted">
            Sessions show up here once they end, so you can look back at what each agent did.
          </p>
        </EmptyState>
      )}
    </div>
  );
}

function HistoryRow({
  session,
  label,
  subagents,
  onOpen,
}: {
  session: Session & { endedAt: string };
  label?: SessionLabel;
  subagents: number;
  onOpen: (sessionId: string) => void;
}) {
  const folder = session.repo?.root ?? session.cwd;
  const tokens = (session.tokens?.input ?? 0) + (session.tokens?.output ?? 0);
  const cost = useCosts().data?.[session.id];
  return (
    <li>
      <button
        type="button"
        onClick={() => onOpen(session.id)}
        className="group flex w-full items-center gap-4 px-4 py-3 text-left transition-colors hover:bg-surface-2"
      >
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5">
            <span className="text-[11px] font-semibold text-muted">{harnessNames[session.harness]}</span>
            <span className="text-[11px] text-subtle" title={folder}>
              {basename(folder)}
            </span>
          </div>
          <div className="mt-0.5 flex min-w-0 items-center gap-2">
            <p className="truncate text-[13px] font-semibold text-ink group-hover:text-accent-text">{sessionTitle(session, label)}</p>
            <LabelBadges label={label} className="shrink-0" />
          </div>
          <div className="mt-1 flex min-w-0 items-center gap-1.5 text-[11px] text-subtle">
            <FolderGit2 size={12} className="shrink-0" aria-hidden />
            <Code className="max-w-96">{session.repo?.branch ?? session.cwd}</Code>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-4 text-[11px] text-muted tabular-nums">
          {subagents > 0 && (
            <span className="inline-flex items-center gap-1" title="Subagents">
              <Network size={12} aria-hidden />
              {subagents}
            </span>
          )}
          {tokens > 0 && (
            <span className="inline-flex items-center gap-1" title="Tokens in and out">
              <Coins size={12} aria-hidden />
              {compactNumber(tokens)}
            </span>
          )}
          {cost != null && (
            <span title="Estimated cost at API list prices">~{usd(cost)}</span>
          )}
          <span className="inline-flex items-center gap-1" title="Duration">
            <Clock size={12} aria-hidden />
            {elapsed(session.startedAt, Date.parse(session.endedAt))}
          </span>
          <span className="w-16 text-right whitespace-nowrap text-subtle">
            {label?.pinned
              ? new Date(session.endedAt).toLocaleDateString([], { month: "short", day: "numeric" })
              : new Date(session.endedAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
          </span>
          <ChevronRight size={14} className="text-subtle group-hover:text-accent-text" aria-hidden />
        </div>
      </button>
    </li>
  );
}
