import { Bot, CircleAlert, Folder, Hand, Layers, Loader, MessageCircle, Moon, Network, Plug, Search, X } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useMemo, useState } from "react";

import type { Agent, AppInfo, Session, SessionLabel } from "../lib/types";
import { SessionCard } from "../components/SessionCard";
import {
  Button,
  Card,
  Dot,
  EmptyState,
  IconTile,
  Label,
  LiveChip,
  Notice,
  PageIntro,
  Pill,
  SectionTitle,
  cx,
  type Tone,
} from "../components/ui";
import { useAppInfo, useLabels, useSnapshot } from "../lib/api";
import { LabelChips, labelText, matchesLabel, pinnedFirst, type LabelFilter } from "../components/Labels";
import {
  countByFilter,
  filterLabels,
  filterOf,
  filterTones,
  liveSessions,
  type SessionFilter,
} from "../lib/filters";
import { basename, harnessNames, needsUser } from "../lib/format";
import { useNow } from "../lib/useNow";

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

function matchesSearch(session: Session, query: string, label?: SessionLabel): boolean {
  if (!query) return true;
  const haystack = [session.title, session.cwd, session.repo?.branch, session.model, harnessNames[session.harness], labelText(label)]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
  return haystack.includes(query.toLowerCase());
}

export function Overview({
  onOpen,
  filter,
  onFilter,
  onSettings,
}: {
  onOpen: (sessionId: string) => void;
  filter: SessionFilter;
  onFilter: (filter: SessionFilter) => void;
  onSettings: () => void;
}) {
  const snapshot = useSnapshot();
  const now = useNow(10_000);
  const [groupBy, setGroupBy] = useState<GroupBy>(loadGroupBy);
  const [query, setQuery] = useState("");
  const [labelFilter, setLabelFilter] = useState<LabelFilter>(null);
  const labels = useLabels().data;

  const chooseGroupBy = (by: GroupBy) => {
    setGroupBy(by);
    try {
      localStorage.setItem(GROUP_BY_KEY, by);
    } catch {
      // Grouping just won't be remembered.
    }
  };

  const live = useMemo(() => liveSessions(snapshot.data?.sessions), [snapshot.data]);
  const agentsBySession = useMemo(() => {
    const map = new Map<string, Agent[]>();
    for (const agent of snapshot.data?.agents ?? []) {
      map.set(agent.sessionId, [...(map.get(agent.sessionId) ?? []), agent]);
    }
    return map;
  }, [snapshot.data]);
  const counts = countByFilter(live);
  const liveSubagents = (snapshot.data?.agents ?? []).filter(
    (a) => a.kind === "subagent" && a.endedAt === null && live.some((s) => s.id === a.sessionId),
  ).length;
  const folders = new Set(live.map((s) => s.repo?.root || s.cwd)).size;

  const visible = pinnedFirst(
    live.filter(
      (s) =>
        (filter === "all" || filterOf(s.state) === filter) &&
        matchesLabel(labelFilter, labels?.get(s.id)) &&
        matchesSearch(s, query, labels?.get(s.id)),
    ),
    labels,
  );
  const needsYou = visible.filter((s) => needsUser(s.state));
  const grouped = new Map<string, Session[]>();
  for (const s of visible.filter((s) => !needsUser(s.state))) {
    const key = groupKey(s, groupBy);
    grouped.set(key, [...(grouped.get(key) ?? []), s]);
  }
  const groups = [...grouped.entries()].sort(([a], [b]) => a.localeCompare(b));

  const card = (s: Session) => (
    <SessionCard
      key={s.id}
      session={s}
      agents={agentsBySession.get(s.id) ?? []}
      concerns={(snapshot.data?.concerns ?? []).filter((c) => c.sessionId === s.id)}
      label={labels?.get(s.id)}
      now={now}
      onOpen={onOpen}
    />
  );
  const toggle = (f: SessionFilter) => onFilter(filter === f ? "all" : f);

  return (
    <div className="mx-auto flex max-w-[1180px] flex-col gap-5">
      <PageIntro
        eyebrow={<LiveChip />}
        title="Your agents"
        subtitle={
          live.length === 0
            ? "Everything AI that runs on this machine, in one view."
            : `${live.length} live session${live.length === 1 ? "" : "s"} across ${folders} folder${folders === 1 ? "" : "s"}.`
        }
        actions={
          <Button icon={Plug} onClick={onSettings}>
            Connect harnesses
          </Button>
        }
      />

      {snapshot.isError && (
        <Notice tone="danger" icon={CircleAlert}>
          Could not load agents: {String(snapshot.error)}
        </Notice>
      )}

      <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-5">
        <StatCard icon={Bot} tone="accent" value={live.length} label="Live sessions" caption={`in ${folders} folder${folders === 1 ? "" : "s"}`} active={filter === "all"} onClick={() => onFilter("all")} />
        <StatCard icon={Hand} tone="warn" value={counts.approval} label="Needs approval" caption="answer in the terminal" active={filter === "approval"} onClick={() => toggle("approval")} />
        <StatCard icon={MessageCircle} tone="ask" value={counts.input} label="Waiting for you" caption="turn finished or asked" active={filter === "input"} onClick={() => toggle("input")} />
        <StatCard icon={Loader} tone="work" value={counts.working} label="Working" caption="thinking or running tools" active={filter === "working"} onClick={() => toggle("working")} />
        <StatCard icon={Network} tone="neutral" value={liveSubagents} label="Subagents" caption="running under sessions" />
      </div>

      {live.length > 0 && (
        <section className="flex flex-col gap-3">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <SectionTitle count={visible.length}>Sessions</SectionTitle>
            <div className="flex flex-wrap items-center gap-2">
              <label className="flex h-8 w-[260px] items-center gap-2 rounded-[9px] border border-line bg-surface px-2.5 focus-within:border-accent focus-within:shadow-[0_0_0_3px_var(--accent-ring)]">
                <Search size={14} className="shrink-0 text-subtle" aria-hidden />
                <input
                  type="search"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  placeholder="Search title, folder, branch or tag"
                  aria-label="Search sessions"
                  className="min-w-0 flex-1 bg-transparent text-[12px] outline-none placeholder:text-subtle [&::-webkit-search-cancel-button]:hidden"
                />
                {query && (
                  <button type="button" aria-label="Clear search" onClick={() => setQuery("")} className="text-subtle hover:text-ink">
                    <X size={13} aria-hidden />
                  </button>
                )}
              </label>
              <div role="group" aria-label="Group sessions by" className="flex h-8 items-center rounded-[9px] border border-line bg-surface p-0.5">
                <span className="px-2 text-[11px] text-subtle">Group:</span>
                {(["repo", "harness"] as const).map((by) => (
                  <button
                    key={by}
                    type="button"
                    aria-pressed={groupBy === by}
                    onClick={() => chooseGroupBy(by)}
                    className={cx(
                      "flex h-full items-center gap-1 rounded-[7px] px-2.5 text-[11px] font-semibold capitalize transition-colors",
                      groupBy === by ? "bg-accent-soft text-accent-text" : "text-muted hover:text-ink",
                    )}
                  >
                    {by === "repo" ? <Folder size={12} aria-hidden /> : <Layers size={12} aria-hidden />}
                    {by}
                  </button>
                ))}
              </div>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-1.5">
            {(Object.keys(filterLabels) as (keyof typeof filterLabels)[]).map((f) => (
              <button
                key={f}
                type="button"
                aria-pressed={filter === f}
                onClick={() => toggle(f)}
                className={cx(
                  "inline-flex h-7 items-center gap-1.5 rounded-full border px-2.5 text-[11px] font-semibold transition-colors",
                  filter === f
                    ? "border-accent-line bg-accent-soft text-accent-text"
                    : "border-line bg-surface text-muted hover:bg-surface-2",
                )}
              >
                <Dot tone={filterTones[f]} />
                {filterLabels[f]}
                <span className="text-subtle">{counts[f]}</span>
              </button>
            ))}
            <LabelChips ids={live.map((s) => s.id)} labels={labels} value={labelFilter} onChange={setLabelFilter} />
            {(filter !== "all" || query || labelFilter) && (
              <button
                type="button"
                onClick={() => {
                  onFilter("all");
                  setQuery("");
                  setLabelFilter(null);
                }}
                className="ml-1 text-[11px] font-semibold text-accent-text hover:underline"
              >
                Clear filters
              </button>
            )}
          </div>

          {needsYou.length > 0 && (
            <Group title="Needs you" tone="warn" count={needsYou.length}>
              {needsYou.map(card)}
            </Group>
          )}
          {groups.map(([name, sessions]) => (
            <Group key={name} title={groupBy === "repo" ? basename(name) : name} fullTitle={name} count={sessions.length}>
              {sessions.map(card)}
            </Group>
          ))}
          {visible.length === 0 && (
            <Card className="px-6 py-10 text-center text-[12px] text-muted">No sessions match these filters.</Card>
          )}
        </section>
      )}

      {snapshot.isSuccess && live.length === 0 && <NoAgents onSettings={onSettings} />}
    </div>
  );
}

function StatCard({
  icon,
  tone,
  value,
  label,
  caption,
  active,
  onClick,
}: {
  icon: LucideIcon;
  tone: Tone;
  value: number;
  label: string;
  caption: string;
  active?: boolean;
  onClick?: () => void;
}) {
  const body = (
    <>
      <IconTile icon={icon} tone={tone} />
      <span className="mt-2 text-[23px] leading-[1.1] font-bold text-ink tabular-nums">{value}</span>
      <span className="text-[12px] font-semibold text-ink-2">{label}</span>
      <span className="text-[10px] text-subtle">{caption}</span>
    </>
  );
  const base = "flex flex-col items-start gap-0.5 rounded-[14px] border bg-surface p-3.5 text-left shadow-card";
  if (!onClick) return <div className={cx(base, "border-line")}>{body}</div>;
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      className={cx(
        base,
        "transition-[background-color,box-shadow,border-color] hover:bg-surface-2",
        active ? "border-accent-line shadow-[0_0_0_3px_var(--accent-ring)]" : "border-line",
      )}
    >
      {body}
    </button>
  );
}

function Group({
  title,
  fullTitle,
  count,
  tone,
  children,
}: {
  title: string;
  fullTitle?: string;
  count: number;
  tone?: Tone;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-2.5 pt-1">
      <div className="flex items-center gap-2" title={fullTitle}>
        {tone && <Dot tone={tone} pulse />}
        <Label className={tone === "warn" ? "text-warn-text" : undefined}>
          {title} · {count}
        </Label>
      </div>
      <div className="flex flex-col gap-2.5">{children}</div>
    </div>
  );
}

function NoAgents({ onSettings }: { onSettings: () => void }) {
  const info = useAppInfo();
  return (
    <EmptyState icon={Moon} title="No agents running">
      <p className="mt-1.5 max-w-md text-[12px] leading-relaxed text-muted">
        Sessions from Claude Code, Codex, Copilot CLI, and Cursor will show up here as soon as they
        start. Connect a harness in Settings to see approvals live.
      </p>
      {info.data && <Harnesses info={info.data} />}
      <Button variant="primary" icon={Plug} className="mt-6" onClick={onSettings}>
        Connect a harness
      </Button>
    </EmptyState>
  );
}

function Harnesses({ info }: { info: AppInfo }) {
  return (
    <ul className="mt-5 flex flex-wrap justify-center gap-2">
      {info.harnesses.map((h) => (
        <li key={h.harness}>
          <Pill tone={h.installed ? "work" : "neutral"} className="px-2.5 py-1 text-[11px]">
            {harnessNames[h.harness]} · {h.installed ? "Detected" : "Not found"}
          </Pill>
        </li>
      ))}
    </ul>
  );
}
