import { Pin, Tag } from "lucide-react";

import type { SessionLabel } from "../lib/types";
import { cx } from "./ui";

/** A label filter: nothing, pinned sessions, or one tag. */
export type LabelFilter = null | { pinned: true } | { tag: string };

export function matchesLabel(filter: LabelFilter, label?: SessionLabel): boolean {
  if (!filter) return true;
  if ("pinned" in filter) return !!label?.pinned;
  return !!label?.tags.includes(filter.tag);
}

/** Text from a label that search should find. */
export function labelText(label?: SessionLabel): string {
  if (!label) return "";
  return [label.name, label.note, ...label.tags].filter(Boolean).join(" ");
}

/** Pinned first, then the order given. */
export function pinnedFirst<T extends { id: string }>(items: T[], labels?: Map<string, SessionLabel>): T[] {
  return [...items].sort((a, b) => Number(!!labels?.get(b.id)?.pinned) - Number(!!labels?.get(a.id)?.pinned));
}

const chip =
  "inline-flex h-7 items-center gap-1.5 rounded-full border px-2.5 text-[11px] font-semibold transition-colors";
const chipOn = "border-accent-line bg-accent-soft text-accent-text";
const chipOff = "border-line bg-surface text-muted hover:bg-surface-2";

/** "Pinned" and one chip per tag used by `ids`. Renders nothing when no session is labelled. */
export function LabelChips({
  ids,
  labels,
  value,
  onChange,
}: {
  ids: string[];
  labels?: Map<string, SessionLabel>;
  value: LabelFilter;
  onChange: (filter: LabelFilter) => void;
}) {
  const used = ids.map((id) => labels?.get(id)).filter((l): l is SessionLabel => !!l);
  const pinned = used.filter((l) => l.pinned).length;
  const tags = new Map<string, number>();
  for (const l of used) for (const t of l.tags) tags.set(t, (tags.get(t) ?? 0) + 1);
  if (pinned === 0 && tags.size === 0) return null;
  const pinnedOn = !!value && "pinned" in value;
  return (
    <>
      {pinned > 0 && (
        <button type="button" aria-pressed={pinnedOn} onClick={() => onChange(pinnedOn ? null : { pinned: true })} className={cx(chip, pinnedOn ? chipOn : chipOff)}>
          <Pin size={11} aria-hidden />
          Pinned
          <span className="text-subtle">{pinned}</span>
        </button>
      )}
      {[...tags.entries()]
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([tag, count]) => {
          const on = !!value && "tag" in value && value.tag === tag;
          return (
            <button key={tag} type="button" aria-pressed={on} onClick={() => onChange(on ? null : { tag })} className={cx(chip, on ? chipOn : chipOff)}>
              <Tag size={11} aria-hidden />
              {tag}
              <span className="text-subtle">{count}</span>
            </button>
          );
        })}
    </>
  );
}

/** A pin and the tags, small, for cards and rows. */
export function LabelBadges({ label, className }: { label?: SessionLabel; className?: string }) {
  if (!label || (!label.pinned && label.tags.length === 0)) return null;
  return (
    <span className={cx("inline-flex min-w-0 flex-wrap items-center gap-1", className)}>
      {label.pinned && (
        <span className="inline-flex items-center text-accent-text" title="Pinned: kept past retention">
          <Pin size={12} className="fill-current" aria-label="Pinned" />
        </span>
      )}
      {label.tags.map((t) => (
        <span key={t} className="rounded-full bg-surface-3 px-1.5 py-px text-[10px] font-semibold text-muted">
          #{t}
        </span>
      ))}
    </span>
  );
}
