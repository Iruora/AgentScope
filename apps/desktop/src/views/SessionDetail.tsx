import { ArrowLeft, CircleAlert, FileText, Network } from "lucide-react";
import { useMemo, useState } from "react";

import { GoToAgentButton } from "../components/GoToAgentButton";
import { StatePill, stateTones } from "../components/StatePill";
import { Button, Card, Code, Dot, Label, Notice, cx, toneSolid, type Tone } from "../components/ui";
import { revealTranscript, useCosts, useSessionEvents, useSnapshot } from "../lib/api";
import { basename, clockTime, compactNumber, elapsed, harnessNames, usd } from "../lib/format";
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
      <div className="mx-auto flex max-w-[1180px] flex-col gap-5">
        <BackButton onBack={onBack} />
        <p className="text-[12px] text-muted">This session is no longer in the history.</p>
      </div>
    );
  }
  if (!session) return null;

  return (
    <div className="mx-auto flex max-w-[1180px] flex-col gap-5">
      <BackButton onBack={onBack} />
      <Header session={session} />
      <div className="grid items-start gap-5 md:grid-cols-[17rem_minmax(0,1fr)]">
        <Card className="min-w-0 p-3">
          <div className="flex items-center gap-2 px-1.5 pt-0.5 pb-2.5">
            <Network size={14} className="text-subtle" aria-hidden />
            <h2 className="text-[13px] font-bold text-ink">Agents</h2>
            <span className="rounded-full bg-surface-3 px-2 py-px text-[11px] font-semibold text-muted">{agents.length}</span>
          </div>
          <AgentTree
            agents={agents}
            rootId={session.rootAgentId}
            selected={selected}
            onSelect={(id) => setSelected((cur) => (cur === id ? null : id))}
          />
        </Card>
        <Card className="min-w-0 overflow-hidden">
          <div className="flex items-center justify-between gap-2 border-b border-line-soft px-4 py-3">
            <h2 className="text-[13px] font-bold text-ink">Timeline</h2>
            {selected && (
              <button type="button" className="text-[11px] font-semibold text-accent-text hover:underline" onClick={() => setSelected(null)}>
                Show all agents
              </button>
            )}
          </div>
          <Timeline session={session} agents={agents} selected={selected} />
        </Card>
      </div>
    </div>
  );
}

function BackButton({ onBack }: { onBack: () => void }) {
  return (
    <button
      type="button"
      onClick={onBack}
      className="inline-flex w-fit items-center gap-1.5 text-[12px] font-semibold text-muted hover:text-ink"
    >
      <ArrowLeft size={14} aria-hidden />
      Back
    </button>
  );
}

function Header({ session }: { session: Session }) {
  const costs = useCosts();
  const [revealError, setRevealError] = useState<string | null>(null);
  const facts: [string, React.ReactNode, string?][] = [
    ["Model", session.model ?? "Unknown"],
    ["Started", `${elapsed(session.startedAt)} ago`],
    ["Folder", <Code key="cwd" className="max-w-full text-[11px]">{session.cwd || "Unknown"}</Code>, session.cwd],
    ["Branch", session.repo?.branch ? <Code key="branch" className="max-w-full text-[11px]">{session.repo.branch}</Code> : "None"],
  ];
  if (session.tokens) {
    const t = session.tokens;
    const cached = t.cached ? ` · ${compactNumber(t.cached)} cached` : "";
    facts.push(["Tokens", `${compactNumber(t.input ?? 0)} in · ${compactNumber(t.output ?? 0)} out${cached}`]);
  }
  const cost = costs.data?.[session.id];
  if (cost != null) facts.push(["Est. cost", `~${usd(cost)}`, "At API list prices; subscription plans are not billed per token"]);
  const folder = session.repo?.root ?? session.cwd;

  return (
    <Card className="overflow-hidden">
      <header className="flex flex-wrap items-start justify-between gap-4 border-b border-line-soft px-5 pt-[18px] pb-3.5">
        <div className="min-w-0">
          <p className="mb-1 text-[11px] font-semibold text-muted">
            {harnessNames[session.harness]}
            <span className="text-subtle"> · {basename(folder)}</span>
          </p>
          <div className="flex flex-wrap items-center gap-2.5">
            <h1 className="truncate text-[19px] font-bold tracking-[-0.3px] text-ink">{session.title ?? "Untitled session"}</h1>
            <StatePill state={session.state} />
          </div>
        </div>
        <div className="flex shrink-0 items-start gap-2">
          {session.transcriptPath && (
            <Button icon={FileText} onClick={() => revealTranscript(session.id).catch((e) => setRevealError(String(e.message ?? e)))}>
              Show transcript
            </Button>
          )}
          {session.endedAt === null && <GoToAgentButton sessionId={session.id} primary />}
        </div>
      </header>
      <dl className="grid grid-cols-2 gap-x-5 gap-y-3.5 px-5 py-4 lg:grid-cols-[1fr_0.8fr_1.4fr_1.3fr_1.7fr]">
        {facts.map(([label, value, title]) => (
          <div key={label} className="min-w-0">
            <dt>
              <Label className="mb-1.5">{label}</Label>
            </dt>
            <dd className="flex min-w-0 text-[12px] text-ink-2" title={title ?? (typeof value === "string" ? value : undefined)}>
              {typeof value === "string" ? <span className="truncate">{value}</span> : value}
            </dd>
          </div>
        ))}
      </dl>
      {revealError && (
        <div className="px-5 pb-4">
          <Notice tone="danger" icon={CircleAlert}>
            {revealError}
          </Notice>
        </div>
      )}
    </Card>
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
        className={cx(
          "flex w-full items-center gap-2 rounded-[9px] py-1.5 pr-2 text-left transition-colors",
          selected === agent.id ? "bg-accent-soft" : "hover:bg-surface-2",
        )}
      >
        <Dot tone={stateTones[agent.state]} />
        <span className="min-w-0 flex-1">
          <span className={cx("block truncate text-[12px] font-semibold", selected === agent.id ? "text-accent-text" : "text-ink-2")}>
            {agentLabel(agent)}
          </span>
          {agent.kind === "subagent" && agent.agentType && agent.description && (
            <span className="block truncate text-[11px] text-subtle">{agent.agentType}</span>
          )}
        </span>
        <StatePill state={agent.state} />
      </button>
      {(children.get(agent.id) ?? []).length > 0 && (
        <ul className="ml-3 border-l border-line-soft">{children.get(agent.id)!.map((c) => node(c, depth + 1))}</ul>
      )}
    </li>
  );
  return <ul className="flex flex-col gap-0.5">{node(root, 0)}</ul>;
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

const toneOf: Record<NonNullable<Row["tone"]>, Tone> = {
  warn: "warn",
  error: "danger",
  muted: "neutral",
};

const textClass = {
  warn: "text-warn-text",
  error: "text-danger-text",
  muted: "text-muted",
};

function Timeline({ session, agents, selected }: { session: Session; agents: Agent[]; selected: string | null }) {
  const events = useSessionEvents(session.id);
  const names = new Map(agents.map((a) => [a.id, agentLabel(a)]));
  const rows = useMemo(() => toRows(session, events.data ?? []), [session, events.data]);
  const visible = selected ? rows.filter((r) => r.agentId === selected) : rows;

  if (events.isError) {
    return (
      <div className="p-4">
        <Notice tone="danger" icon={CircleAlert}>
          Could not load events: {String(events.error)}
        </Notice>
      </div>
    );
  }
  if (events.isSuccess && visible.length === 0) {
    return <p className="px-4 py-8 text-center text-[12px] text-muted">No events yet.</p>;
  }
  return (
    <ol className="divide-y divide-line-soft">
      {[...visible].reverse().map((r) => (
        <li key={r.key} className="flex items-baseline gap-3 px-4 py-2.5 hover:bg-surface-2">
          <time className="w-[84px] shrink-0 font-mono text-[11px] whitespace-nowrap text-subtle" dateTime={r.at}>
            {clockTime(r.at)}
          </time>
          <span className={cx("size-[7px] shrink-0 translate-y-[-1px] rounded-full", r.tone ? toneSolid[toneOf[r.tone]] : "bg-accent")} aria-hidden />
          <div className="min-w-0 flex-1">
            <p className={cx("truncate text-[12px]", r.tone ? textClass[r.tone] : "text-ink-2")} title={r.text}>
              {r.text}
            </p>
            {!selected && r.agentId !== session.rootAgentId && (
              <p className="truncate text-[11px] text-subtle">{names.get(r.agentId) ?? "Subagent"}</p>
            )}
          </div>
          {r.detail && (
            <span
              className={cx(
                "shrink-0 rounded-[6px] px-1.5 py-px text-[10px] font-semibold",
                r.detail === "running" ? "bg-work-soft text-work-text" : r.tone === "error" ? "bg-danger-soft text-danger-text" : "bg-surface-3 text-muted",
              )}
            >
              {r.detail}
            </span>
          )}
        </li>
      ))}
    </ol>
  );
}
