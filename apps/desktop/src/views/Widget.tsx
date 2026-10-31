import { LogicalSize, getCurrentWindow } from "@tauri-apps/api/window";
import { Hand, Radar, X } from "lucide-react";
import { useEffect, useRef } from "react";

import { stateTones } from "../components/StatePill";
import { Dot, cx } from "../components/ui";
import { sessionTitle, setWidget, showSession, useGraphUpdates, useLabels, useSnapshot } from "../lib/api";
import { liveSessions } from "../lib/filters";
import { basename, elapsed, needsUser, stateLabels } from "../lib/format";
import { useNow } from "../lib/useNow";

/** Rows shown before "+N more". */
const MAX_ROWS = 6;
const WIDTH = 280;

/**
 * The mini widget: an always-on-top strip with the "Needs you" count and one
 * line per live agent. Drag it by the header; click a line to open it.
 */
export function Widget() {
  useGraphUpdates();
  const snapshot = useSnapshot();
  const labels = useLabels().data;
  const now = useNow(10_000);
  const root = useRef<HTMLDivElement>(null);

  const live = liveSessions(snapshot.data?.sessions).sort(
    (a, b) => Number(needsUser(b.state)) - Number(needsUser(a.state)),
  );
  const waiting = live.filter((s) => needsUser(s.state)).length;
  const rows = live.slice(0, MAX_ROWS);

  // The window fits its content, so it never shows empty space or a scrollbar.
  useEffect(() => {
    const el = root.current;
    if (!el) return;
    const fit = () => void getCurrentWindow().setSize(new LogicalSize(WIDTH, Math.ceil(el.scrollHeight)));
    const observer = new ResizeObserver(fit);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  return (
    <div ref={root} className="flex flex-col overflow-hidden border border-line bg-bg text-ink select-none">
      <header data-tauri-drag-region className="flex cursor-grab items-center gap-2 border-b border-line-soft px-3 py-2">
        <Radar size={14} className="pointer-events-none text-accent-text" aria-hidden />
        <span className="pointer-events-none flex-1 text-[12px] font-bold">AgentScope</span>
        <span
          className={cx(
            "pointer-events-none inline-flex items-center gap-1 rounded-full px-2 py-[3px] text-[10px] leading-none font-semibold",
            waiting > 0 ? "bg-warn-soft text-warn-text" : "bg-surface-3 text-muted",
          )}
        >
          {waiting > 0 && <Hand size={11} aria-hidden />}
          {waiting > 0 ? `${waiting} need${waiting === 1 ? "s" : ""} you` : "All clear"}
        </span>
        <button
          type="button"
          aria-label="Hide the widget"
          title="Hide the widget"
          onClick={() => void setWidget(false)}
          className="grid size-6 place-items-center rounded-[6px] text-subtle hover:bg-surface-2 hover:text-ink"
        >
          <X size={13} aria-hidden />
        </button>
      </header>
      {rows.length === 0 ? (
        <p className="px-3 py-3 text-[11px] text-subtle">No agents running.</p>
      ) : (
        <ul className="flex flex-col py-1">
          {rows.map((s) => (
            <li key={s.id}>
              <button
                type="button"
                onClick={() => void showSession(s.id)}
                title={`${stateLabels[s.state]} · ${s.repo?.root ?? s.cwd}`}
                className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-surface-2"
              >
                <Dot tone={stateTones[s.state]} pulse={needsUser(s.state)} />
                <span className="min-w-0 flex-1 truncate text-[12px] text-ink-2">{sessionTitle(s, labels?.get(s.id))}</span>
                <span className="shrink-0 text-[10px] text-subtle tabular-nums">
                  {needsUser(s.state) ? stateLabels[s.state] : `${basename(s.repo?.root ?? s.cwd)} · ${elapsed(s.startedAt, now)}`}
                </span>
              </button>
            </li>
          ))}
          {live.length > MAX_ROWS && (
            <li className="px-3 pt-0.5 pb-1 text-[10px] text-subtle">+{live.length - MAX_ROWS} more</li>
          )}
        </ul>
      )}
    </div>
  );
}
