import { FileWarning, Hourglass, MoonStar, Repeat } from "lucide-react";
import type { LucideIcon } from "lucide-react";

import type { Concern, ConcernKind } from "../lib/types";
import { cx } from "./ui";

export const concernIcons: Record<ConcernKind, LucideIcon> = {
  long_tool: Hourglass,
  repeated_failure: Repeat,
  silent: MoonStar,
  conflict: FileWarning,
};

export const concernTitles: Record<ConcernKind, string> = {
  long_tool: "Long-running tool",
  repeated_failure: "Failing in a loop",
  silent: "Gone quiet",
  conflict: "Possible conflict",
};

/** One soft warning line per concern, for cards. */
export function ConcernLines({ concerns, max = 2, className }: { concerns: Concern[]; max?: number; className?: string }) {
  if (concerns.length === 0) return null;
  return (
    <ul className={cx("flex flex-col gap-1", className)}>
      {concerns.slice(0, max).map((c) => {
        const Icon = concernIcons[c.kind];
        return (
          <li key={c.id} className="flex min-w-0 items-center gap-1.5 text-[11.5px] text-warn-text" title={`${concernTitles[c.kind]}: ${c.message}`}>
            <Icon size={13} className="shrink-0" aria-hidden />
            <span className="sr-only">{concernTitles[c.kind]}: </span>
            <span className="min-w-0 truncate">{c.message}</span>
          </li>
        );
      })}
      {concerns.length > max && <li className="text-[11px] text-subtle">+{concerns.length - max} more</li>}
    </ul>
  );
}
