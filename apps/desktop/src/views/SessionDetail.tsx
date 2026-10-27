import { useMemo, useState } from "react";

import { GoToAgentButton } from "../components/GoToAgentButton";
import { StatePill } from "../components/StatePill";
import { revealTranscript, useSessionEvents, useSnapshot } from "../lib/api";
import { clockTime, compactNumber, elapsed, harnessNames } from "../lib/format";
import type { Agent, AgentEvent, Session } from "../lib/types";

export function SessionDetail({ sessionId, onBack }: { sessionId: string; onBack: () => void }) {
  const snapshot = useSnapshot();
  const session = snapshot.data?.sessions.find((s) => s.id === sessionId);
  const agents = useMemo(
    () => (snapshot.data?.agents ?? []).filter((a) => a.sessionId === sessionId),
    [snapshot.data, sessionId],
  );
  const [selected, setSelected] = useState<string | null>(null);

  if (snapshot.isSuccess && !session) {
    return (
      <div className="mx-auto max-w-5xl px-6 py-6">
        <BackButton onBack={onBack} />
        <p className="mt-6 text-sm text-zinc-500">This session is no longer in the history.</p>
      </div>
    );
  }
  if (!session) return null;

  return (
    <div className="mx-auto max-w-6xl px-6 py-6">
      <BackButton onBack={onBack} />
      <Header session={session} />
      <div className="mt-6 grid gap-6 md:grid-cols-[16rem_1fr]">
        <aside className="min-w-0">
          <h2 className="mb-2 text-sm font-semibold text-zinc-500">Agents</h2>
          <AgentTree
            agents={agents}
            rootId={session.rootAgentId}
            selected={selected}
            onSelect={(id) => setSelected((cur) => (cur === id ? null : id))}
          />
        </aside>
        <section className="min-w-0">
          <h2 className="mb-2 text-sm font-semibold text-zinc-500">
            Timeline
            {selected && (
              <button type="button" className="ml-2 font-normal text-sky-700 hover:underline dark:text-sky-400" onClick={() => setSelected(null)}>
                Show all agents
              </button>
            )}
          </h2>
          <Timeline session={session} agents={agents} selected={selected} />
        </section>
      </div>
    </div>
  );
}

function BackButton({ onBack }: { onBack: () => void }) {
  return (
    <button type="button" onClick={onBack} className="text-sm text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-100">
      ← Back
    </button>
  );
}

function Header({ session }: { session: Session }) {
  const [revealError, setRevealError] = useState<string | null>(null);
  const facts: [string, string][] = [
    ["Harness", harnessNames[session.harness]],
    ["Model", session.model ?? "Unknown"],
    ["Folder", session.cwd || "Unknown"],
    ["Branch", session.repo?.branch ?? "None"],
    ["Started", `${elapsed(session.startedAt)} ago`],
  ];
  if (session.tokens) {
    const t = session.tokens;
    const cached = t.cached ? ` · ${compactNumber(t.cached)} cached` : "";
    facts.push(["Tokens", `${compactNumber(t.input ?? 0)} in · ${compactNumber(t.output ?? 0)} out${cached}`]);
  }

  return (
    <header className="mt-4 rounded-xl border border-zinc-200 bg-white p-5 dark:border-zinc-800 dark:bg-zinc-900">
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <h1 className="truncate text-xl font-semibold">{session.title ?? "Untitled session"}</h1>
          <div className="mt-1">
            <StatePill state={session.state} />
          </div>
        </div>
        <div className="flex shrink-0 items-start gap-2">
          {session.transcriptPath && (
            <button
              type="button"
              onClick={() => revealTranscript(session.id).catch((e) => setRevealError(String(e.message ?? e)))}
              className="rounded-md border border-zinc-300 px-2.5 py-1 text-xs font-medium hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800"
            >
              Show transcript
            </button>
          )}
          {session.endedAt === null && <GoToAgentButton sessionId={session.id} primary />}
        </div>
      </div>
      {revealError && <p className="mt-2 text-xs text-red-600">{revealError}</p>}
      <dl className="mt-4 grid grid-cols-[auto_1fr] gap-x-6 gap-y-1 text-sm sm:grid-cols-[auto_1fr_auto_1fr]">
        {facts.map(([label, value]) => (
          <div key={label} className="contents">
            <dt className="text-zinc-500">{label}</dt>
            <dd className="truncate" title={value}>
              {value}
            </dd>
          </div>
        ))}
      </dl>
    </header>
  );
}

function agentLabel(agent: Agent): string {
  if (agent.kind === "main") return "Main agent";
  return agent.description ?? agent.agentType ?? "Subagent";
}

function AgentTree({
  agents,
  rootId,
  selected,
  onSelect,
}: {
  agents: Agent[];
  rootId: string;
  selected: string | null;
  onSelect: (id: string) => void;
}) {
  const children = useMemo(() => {
    const map = new Map<string, Agent[]>();
    for (const a of agents) {
      if (!a.parentAgentId) continue;
      map.set(a.parentAgentId, [...(map.get(a.parentAgentId) ?? []), a]);
    }
    return map;
  }, [agents]);
  const root = agents.find((a) => a.id === rootId);
  if (!root) return null;

  const node = (agent: Agent, depth: number) => (
    <li key={agent.id}>
      <button
        type="button"
        aria-pressed={selected === agent.id}
        onClick={() => onSelect(agent.id)}
        style={{ paddingLeft: `${depth * 0.875 + 0.5}rem` }}
        className={`flex w-full items-center justify-between gap-2 rounded-md py-1.5 pr-2 text-left text-sm ${
          selected === agent.id ? "bg-zinc-200/70 dark:bg-zinc-800" : "hover:bg-zinc-100 dark:hover:bg-zinc-900"
        }`}
      >
        <span className="min-w-0">
          <span className="block truncate">{agentLabel(agent)}</span>
          {agent.kind === "subagent" && agent.agentType && agent.description && (
            <span className="block truncate text-xs text-zinc-500">{agent.agentType}</span>
          )}
        </span>
        <StatePill state={agent.state} />
      </button>
      {(children.get(agent.id) ?? []).length > 0 && (
        <ul>{children.get(agent.id)!.map((c) => node(c, depth + 1))}</ul>
      )}
    </li>
  );
  return <ul className="space-y-0.5">{node(root, 0)}</ul>;
}

type Row = {
  key: string;
  at: string;
  agentId: string;
  text: string;
  tone?: "warn" | "error" | "muted";
  detail?: string;
};

/** Turns stored events into timeline rows, pairing each tool call with its result. */
function toRows(session: Session, events: AgentEvent[]): Row[] {
  const agentId = (e: AgentEvent) => `${session.id}/${e.agent ?? "main"}`;
  const results = new Map<string, AgentEvent>();
  for (const e of events) {
    if ((e.kind.type === "toolFinished" || e.kind.type === "toolFailed") && e.dedupeKey) {
      results.set(e.dedupeKey, e);
    }
  }
  const rows: Row[] = [];
  events.forEach((e, i) => {
    const base = { key: `${i}`, at: e.at, agentId: agentId(e) };
    const k = e.kind;
    switch (k.type) {
      case "sessionStarted":
        rows.push({ ...base, text: "Session started", tone: "muted" });
        break;
      case "sessionEnded":
        rows.push({ ...base, text: "Session ended", tone: "muted" });
        break;
      case "promptSubmitted":
        rows.push({ ...base, text: `Prompt: ${k.prompt}` });
        break;
      case "agentStarted":
        rows.push({
          ...base,
          text: `Subagent started${k.agentType ? ` (${k.agentType})` : ""}${k.description ? `: ${k.description}` : ""}`,
        });
        break;
      case "agentStopped":
        rows.push({ ...base, text: `Subagent ${k.result}`, tone: "muted" });
        break;
      case "toolStarted": {
        const result = e.dedupeKey ? results.get(e.dedupeKey) : undefined;
        const seconds = result ? Math.max(0, (Date.parse(result.at) - Date.parse(e.at)) / 1000) : null;
        const failed = result?.kind.type === "toolFailed";
        rows.push({
          ...base,
          text: k.summary,
          tone: failed ? "error" : undefined,
          detail: seconds === null ? "running" : `${failed ? "failed · " : ""}${seconds.toFixed(1)}s`,
        });
        break;
      }
      case "toolFailed":
        if (!e.dedupeKey && k.error) rows.push({ ...base, text: `Tool failed: ${k.error}`, tone: "error" });
        break;
      case "approvalRequested":
        rows.push({ ...base, text: `Asked for approval: ${k.summary}`, tone: "warn" });
        break;
      case "approvalResolved":
        rows.push({ ...base, text: `Approval ${k.resolution}`, tone: "muted" });
        break;
      case "waitingForInput":
        rows.push({ ...base, text: "Waiting for input", tone: "warn" });
        break;
      case "turnCompleted":
        rows.push({ ...base, text: "Turn finished", tone: "muted" });
        break;
      case "compacted":
        rows.push({ ...base, text: "Compacted context", tone: "muted" });
        break;
      default:
        break;
    }
  });
  // Transcripts are read file by file, so storage order is not time order.
  return rows.sort((a, b) => Date.parse(a.at) - Date.parse(b.at));
}

const toneClass = {
  warn: "text-amber-700 dark:text-amber-400",
  error: "text-red-700 dark:text-red-400",
  muted: "text-zinc-500",
};

function Timeline({ session, agents, selected }: { session: Session; agents: Agent[]; selected: string | null }) {
  const events = useSessionEvents(session.id);
  const names = new Map(agents.map((a) => [a.id, agentLabel(a)]));
  const rows = useMemo(() => toRows(session, events.data ?? []), [session, events.data]);
  const visible = selected ? rows.filter((r) => r.agentId === selected) : rows;

  if (events.isError) {
    return <p className="text-sm text-red-600">Could not load events: {String(events.error)}</p>;
  }
  if (events.isSuccess && visible.length === 0) {
    return <p className="text-sm text-zinc-500">No events yet.</p>;
  }
  return (
    <ol className="divide-y divide-zinc-200 rounded-xl border border-zinc-200 bg-white dark:divide-zinc-800 dark:border-zinc-800 dark:bg-zinc-900">
      {[...visible].reverse().map((r) => (
        <li key={r.key} className="flex items-baseline gap-3 px-4 py-2 text-sm">
          <time className="w-20 shrink-0 font-mono text-xs text-zinc-500" dateTime={r.at}>
            {clockTime(r.at)}
          </time>
          <div className="min-w-0 flex-1">
            <p className={`truncate ${r.tone ? toneClass[r.tone] : ""}`} title={r.text}>
              {r.text}
            </p>
            {!selected && r.agentId !== session.rootAgentId && (
              <p className="truncate text-xs text-zinc-500">{names.get(r.agentId) ?? "Subagent"}</p>
            )}
          </div>
          {r.detail && <span className="shrink-0 text-xs text-zinc-500">{r.detail}</span>}
        </li>
      ))}
    </ol>
  );
}
