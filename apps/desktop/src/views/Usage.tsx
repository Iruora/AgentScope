import { CircleAlert, Coins, Info } from "lucide-react";
import { useState } from "react";

import { Card, Notice, PageIntro, SectionTitle, cx } from "../components/ui";
import { useUsage } from "../lib/api";
import { basename, compactNumber, harnessNames, usd } from "../lib/format";
import type { HarnessId, UsageTotal as RawTotal } from "../lib/types";

/** Specta types every f64 as `number | null`; the core never sends null. */
type UsageTotal = { key: string; input: number; output: number; cached: number; cost: number; unpricedTokens: number };
const clean = (t: RawTotal): UsageTotal => ({
  key: t.key,
  input: t.input ?? 0,
  output: t.output ?? 0,
  cached: t.cached ?? 0,
  cost: t.cost ?? 0,
  unpricedTokens: t.unpricedTokens ?? 0,
});

const RANGES = [7, 30, 90] as const;

/** Tokens used and their estimated API cost, from what the harnesses report. */
export function Usage() {
  const [days, setDays] = useState<(typeof RANGES)[number]>(30);
  const usage = useUsage(days);
  const raw = usage.data;
  const report = raw && {
    ...raw,
    total: clean(raw.total),
    byDay: raw.byDay.map(clean),
    byHarness: raw.byHarness.map(clean),
    byRepo: raw.byRepo.map(clean),
    byModel: raw.byModel.map(clean),
  };
  const priced = report ? report.total.cost > 0 : false;

  return (
    <div className="mx-auto flex max-w-[1180px] flex-col gap-5">
      <PageIntro
        title="Usage"
        subtitle="Tokens your agents used and what they would cost at API list prices. On a subscription plan you are not billed per token, so treat cost as an estimate."
        actions={
          <div role="radiogroup" aria-label="Range" className="flex rounded-[9px] border border-line bg-surface p-0.5">
            {RANGES.map((d) => (
              <button
                key={d}
                type="button"
                role="radio"
                aria-checked={days === d}
                onClick={() => setDays(d)}
                className={cx(
                  "h-7 rounded-[7px] px-3 text-[12px] font-semibold transition-colors",
                  days === d ? "bg-accent-soft text-accent-text" : "text-muted hover:text-ink",
                )}
              >
                {d} days
              </button>
            ))}
          </div>
        }
      />

      {usage.isError && (
        <Notice tone="danger" icon={CircleAlert}>
          {usage.error.message}
        </Notice>
      )}
      {report?.pricesError && (
        <Notice tone="warn" icon={CircleAlert}>
          Your prices.json could not be read, so the bundled prices are used: {report.pricesError}
        </Notice>
      )}

      {report && (
        <>
          <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
            <Stat label="Estimated cost" value={usd(report.total.cost)} hint={report.total.unpricedTokens > 0 ? `plus ${compactNumber(report.total.unpricedTokens)} tokens without a price` : undefined} />
            <Stat label="Tokens in" value={compactNumber(report.total.input)} />
            <Stat label="Tokens out" value={compactNumber(report.total.output)} />
            <Stat label="Cache reads" value={compactNumber(report.total.cached)} />
          </div>

          <Card className="px-5 py-4">
            <SectionTitle>{priced ? "Estimated cost per day" : "Tokens per day"}</SectionTitle>
            <DayChart days={report.byDay} metric={priced ? "cost" : "tokens"} />
          </Card>

          <div className="grid gap-3 lg:grid-cols-3">
            <Ranked title="By harness" rows={report.byHarness} label={(k) => harnessNames[k as HarnessId] ?? k} />
            <Ranked title="By repository" rows={report.byRepo} label={(k) => (k.startsWith("(") ? "No repository" : basename(k))} />
            <Ranked title="By model" rows={report.byModel} label={(k) => k} />
          </div>

          <p className="flex items-start gap-1.5 text-[11px] text-subtle">
            <Info size={12} className="mt-px shrink-0" aria-hidden />
            <span>
              Prices checked {report.pricesChecked}.
              {report.unpricedModels.length > 0 && ` No price for ${report.unpricedModels.join(", ")}.`} Add or change prices in a prices.json file in
              AgentScope's data folder. Daily totals are kept for 90 days, longer than session history.
            </span>
          </p>
        </>
      )}
    </div>
  );
}

function Stat({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return (
    <Card className="px-4 py-3.5">
      <p className="text-[11px] font-semibold text-muted">{label}</p>
      <p className="mt-1 text-[24px] leading-none font-bold tracking-[-0.5px] text-ink tabular-nums">{value}</p>
      {hint && <p className="mt-1.5 text-[11px] text-subtle">{hint}</p>}
    </Card>
  );
}

const value = (t: UsageTotal, metric: "cost" | "tokens") => (metric === "cost" ? t.cost : t.input + t.output);
const fmt = (n: number, metric: "cost" | "tokens") => (metric === "cost" ? usd(n) : compactNumber(n));

function shortDay(day: string): string {
  const [y, m, d] = day.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString([], { month: "short", day: "numeric" });
}

/** One bar per day; hover or focus a bar for its value. */
function DayChart({ days, metric }: { days: UsageTotal[]; metric: "cost" | "tokens" }) {
  const [hover, setHover] = useState<number | null>(null);
  const max = Math.max(...days.map((d) => value(d, metric)), 0);
  const top = max > 0 ? niceCeil(max) : 1;
  const shown = hover != null ? days[hover] : null;
  const labelEvery = Math.ceil(days.length / 8);

  return (
    <div className="mt-3">
      <div className="mb-2 h-4 text-[11px] text-muted tabular-nums">
        {shown ? (
          <>
            <span className="font-semibold text-ink">{shortDay(shown.key)}</span> · {fmt(value(shown, metric), metric)}
            {metric === "cost" && ` · ${compactNumber(shown.input + shown.output)} tokens`}
          </>
        ) : (
          "Hover a day for its total"
        )}
      </div>
      <div className="flex gap-2">
        <div className="flex h-40 flex-col justify-between text-right text-[10px] text-subtle tabular-nums" aria-hidden>
          <span>{fmt(top, metric)}</span>
          <span>{fmt(top / 2, metric)}</span>
          <span>{fmt(0, metric)}</span>
        </div>
        <div className="relative h-40 flex-1">
          <div aria-hidden className="absolute inset-x-0 top-0 border-t border-line-soft" />
          <div aria-hidden className="absolute inset-x-0 top-1/2 border-t border-line-soft" />
          <div aria-hidden className="absolute inset-x-0 bottom-0 border-t border-line" />
          <ul className="absolute inset-0 flex items-end gap-[2px]" onMouseLeave={() => setHover(null)}>
            {days.map((d, i) => {
              const v = value(d, metric);
              return (
                <li key={d.key} className="flex h-full flex-1 items-end">
                  <button
                    type="button"
                    aria-label={`${shortDay(d.key)}: ${fmt(v, metric)}`}
                    onMouseEnter={() => setHover(i)}
                    onFocus={() => setHover(i)}
                    onBlur={() => setHover(null)}
                    className="flex h-full w-full items-end"
                  >
                    <span
                      className={cx("w-full rounded-t-[4px] transition-colors", hover === i ? "bg-accent-strong" : "bg-accent")}
                      style={{ height: v > 0 ? `max(2px, ${(v / top) * 100}%)` : 0 }}
                    />
                  </button>
                </li>
              );
            })}
          </ul>
        </div>
      </div>
      <div className="mt-1.5 ml-10 flex text-[10px] text-subtle" aria-hidden>
        {days.map((d, i) => (
          <span key={d.key} className="flex flex-1 justify-center overflow-visible whitespace-nowrap">
            {i % labelEvery === 0 ? shortDay(d.key) : ""}
          </span>
        ))}
      </div>
    </div>
  );
}

/** Rounds up to 1, 2 or 5 times a power of ten. */
function niceCeil(n: number): number {
  const p = 10 ** Math.floor(Math.log10(n));
  for (const m of [1, 2, 5, 10]) if (m * p >= n) return m * p;
  return 10 * p;
}

function Ranked({ title, rows, label }: { title: string; rows: UsageTotal[]; label: (key: string) => string }) {
  const metric = rows.some((r) => r.cost > 0) ? "cost" : "tokens";
  const max = Math.max(...rows.map((r) => value(r, metric)), 0);
  return (
    <Card className="px-5 py-4">
      <SectionTitle>{title}</SectionTitle>
      {rows.length === 0 ? (
        <p className="mt-3 flex items-center gap-1.5 text-[12px] text-muted">
          <Coins size={13} aria-hidden /> No usage in this range
        </p>
      ) : (
        <ul className="mt-3 flex flex-col gap-2.5">
          {rows.slice(0, 8).map((r) => (
            <li key={r.key}>
              <div className="flex items-baseline justify-between gap-3 text-[12px]">
                <span className="min-w-0 truncate font-semibold text-ink" title={r.key}>
                  {label(r.key)}
                </span>
                <span className="shrink-0 text-muted tabular-nums">
                  {r.cost > 0 ? usd(r.cost) : "–"}
                  <span className="ml-2 text-subtle">{compactNumber(r.input + r.output)}</span>
                </span>
              </div>
              <div className="mt-1 h-1.5 rounded-full bg-surface-2">
                <div className="h-full rounded-full bg-accent" style={{ width: max > 0 ? `${(value(r, metric) / max) * 100}%` : 0 }} />
              </div>
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
}
