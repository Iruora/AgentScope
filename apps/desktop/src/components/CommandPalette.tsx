import { CornerDownLeft, Search } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { Dot, cx, type Tone } from "./ui";

export type PaletteItem = {
  id: string;
  group: string;
  label: string;
  /** Extra words that match, e.g. repo, branch, harness. */
  keywords?: string;
  detail?: string;
  icon?: LucideIcon;
  tone?: Tone;
  run: () => void;
  /** Mod+Enter: a second action, e.g. Go to agent for a session. */
  alt?: () => void;
};

export const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
export const modKey = isMac ? "⌘" : "Ctrl";

/** Items whose text contains every word of the query, in their original order. */
export function filterItems(items: PaletteItem[], query: string): PaletteItem[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return items;
  return items.filter((item) => {
    const text = `${item.label} ${item.keywords ?? ""} ${item.detail ?? ""} ${item.group}`.toLowerCase();
    return words.every((w) => text.includes(w));
  });
}

export function CommandPalette({ items, onClose }: { items: PaletteItem[]; onClose: () => void }) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const list = useRef<HTMLUListElement>(null);
  const shown = useMemo(() => filterItems(items, query), [items, query]);
  const current = Math.min(active, Math.max(shown.length - 1, 0));

  useEffect(() => {
    list.current?.querySelector(`[data-index="${current}"]`)?.scrollIntoView({ block: "nearest" });
  }, [current]);

  const choose = (item: PaletteItem | undefined, alt: boolean) => {
    if (!item) return;
    onClose();
    (alt && item.alt ? item.alt : item.run)();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((current + 1) % Math.max(shown.length, 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((current - 1 + shown.length) % Math.max(shown.length, 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      choose(shown[current], e.metaKey || e.ctrlKey);
    } else if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    }
  };

  let lastGroup = "";
  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-[rgb(2_6_23/0.45)] px-4 pt-[12vh]"
      onMouseDown={onClose}
      role="presentation"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        className="flex max-h-[min(560px,72vh)] w-full max-w-[600px] flex-col overflow-hidden rounded-[14px] border border-line bg-surface shadow-[0_24px_60px_rgb(2_6_23/0.35)]"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <label className="flex items-center gap-2.5 border-b border-line-soft px-4">
          <Search size={16} className="shrink-0 text-subtle" aria-hidden />
          <input
            autoFocus
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setActive(0);
            }}
            onKeyDown={onKeyDown}
            placeholder="Search sessions, views and actions"
            aria-label="Search"
            aria-controls="palette-list"
            aria-activedescendant={shown[current] ? `palette-${shown[current].id}` : undefined}
            className="h-12 min-w-0 flex-1 bg-transparent text-[14px] text-ink outline-none placeholder:text-subtle"
            style={{ outline: "none" }}
          />
        </label>
        <ul ref={list} id="palette-list" role="listbox" className="min-h-0 flex-1 overflow-y-auto p-1.5">
          {shown.length === 0 && <li className="px-3 py-6 text-center text-[12px] text-muted">Nothing matches “{query}”.</li>}
          {shown.map((item, i) => {
            const header = item.group !== lastGroup;
            lastGroup = item.group;
            const Icon = item.icon;
            return (
              <li key={item.id} role="none">
                {header && (
                  <p className="px-2.5 pt-2.5 pb-1 text-[10px] font-bold tracking-[0.6px] text-subtle uppercase">{item.group}</p>
                )}
                <div
                  id={`palette-${item.id}`}
                  role="option"
                  aria-selected={i === current}
                  data-index={i}
                  onMouseMove={() => setActive(i)}
                  onClick={(e) => choose(item, e.metaKey || e.ctrlKey)}
                  className={cx(
                    "flex cursor-default items-center gap-2.5 rounded-[9px] px-2.5 py-2 text-[13px]",
                    i === current ? "bg-accent-soft text-accent-text" : "text-ink",
                  )}
                >
                  {item.tone ? <Dot tone={item.tone} /> : Icon ? <Icon size={15} aria-hidden className="shrink-0 text-muted" /> : null}
                  <span className="min-w-0 flex-1 truncate font-semibold">{item.label}</span>
                  {item.detail && <span className="max-w-[45%] shrink-0 truncate text-[11px] text-muted">{item.detail}</span>}
                  {i === current && <CornerDownLeft size={13} aria-hidden className="shrink-0 opacity-70" />}
                </div>
              </li>
            );
          })}
        </ul>
        <footer className="flex flex-wrap gap-x-4 gap-y-1 border-t border-line-soft px-4 py-2 text-[11px] text-subtle">
          <span>↑↓ to move</span>
          <span>↵ to open</span>
          <span>{modKey}+↵ to go to the agent</span>
          <span>Esc to close</span>
        </footer>
      </div>
    </div>
  );
}
