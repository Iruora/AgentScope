import type { LucideIcon } from "lucide-react";
import type { ButtonHTMLAttributes, ReactNode } from "react";

export type Tone = "accent" | "work" | "warn" | "ask" | "danger" | "neutral";

/** Soft background and text, for pills and icon tiles. */
export const toneSoft: Record<Tone, string> = {
  accent: "bg-accent-soft text-accent-text",
  work: "bg-work-soft text-work-text",
  warn: "bg-warn-soft text-warn-text",
  ask: "bg-ask-soft text-ask-text",
  danger: "bg-danger-soft text-danger-text",
  neutral: "bg-neutral-soft text-neutral-text",
};

export const toneText: Record<Tone, string> = {
  accent: "text-accent-text",
  work: "text-work-text",
  warn: "text-warn-text",
  ask: "text-ask-text",
  danger: "text-danger-text",
  neutral: "text-neutral-text",
};

/** Solid colour, for dots and stripes. */
export const toneSolid: Record<Tone, string> = {
  accent: "bg-accent",
  work: "bg-work",
  warn: "bg-warn",
  ask: "bg-ask",
  danger: "bg-danger",
  neutral: "bg-neutral",
};

export function cx(...classes: (string | false | null | undefined)[]): string {
  return classes.filter(Boolean).join(" ");
}

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary" | "ghost";
  size?: "sm" | "md";
  icon?: LucideIcon;
};

export function Button({ variant = "secondary", size = "md", icon: Icon, className, children, ...rest }: ButtonProps) {
  return (
    <button
      type="button"
      {...rest}
      className={cx(
        "inline-flex shrink-0 items-center justify-center gap-1.5 font-semibold whitespace-nowrap transition-colors disabled:cursor-default disabled:opacity-55",
        size === "sm" ? "h-7 rounded-[8px] px-2.5 text-[11px]" : "h-[34px] rounded-[9px] px-3.5 text-[12px]",
        variant === "primary" &&
          "bg-accent text-white shadow-glow hover:enabled:bg-accent-strong dark:text-[#04161d]",
        variant === "secondary" && "border border-line bg-surface text-ink-2 hover:enabled:bg-surface-2",
        variant === "ghost" && "text-muted hover:enabled:bg-surface-2 hover:enabled:text-ink",
        className,
      )}
    >
      {Icon && <Icon size={size === "sm" ? 13 : 14} strokeWidth={2.2} aria-hidden />}
      {children}
    </button>
  );
}

export function IconButton({
  icon: Icon,
  label,
  badge,
  className,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { icon: LucideIcon; label: string; badge?: number }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      {...rest}
      className={cx(
        "relative grid size-[30px] shrink-0 place-items-center rounded-[8px] border border-line bg-surface text-muted transition-colors hover:enabled:bg-surface-2 hover:enabled:text-ink disabled:opacity-50",
        className,
      )}
    >
      <Icon size={15} strokeWidth={2} aria-hidden />
      {badge != null && badge > 0 && (
        <span className="absolute -top-1.5 -right-1.5 grid h-4 min-w-4 place-items-center rounded-full bg-warn px-1 text-[9px] font-bold text-[#2b1d00] ring-2 ring-surface">
          {badge}
        </span>
      )}
    </button>
  );
}

export function Pill({
  tone = "neutral",
  icon: Icon,
  dashed = false,
  spin = false,
  className,
  children,
}: {
  tone?: Tone;
  icon?: LucideIcon;
  dashed?: boolean;
  spin?: boolean;
  className?: string;
  children: ReactNode;
}) {
  return (
    <span
      className={cx(
        "inline-flex shrink-0 items-center gap-1 rounded-full px-2 py-[3px] text-[10px] leading-none font-semibold whitespace-nowrap",
        dashed ? cx("border border-dashed border-current", toneText[tone]) : toneSoft[tone],
        className,
      )}
    >
      {Icon && <Icon size={11} strokeWidth={2.4} className={spin ? "animate-spin [animation-duration:1.4s]" : ""} aria-hidden />}
      {children}
    </span>
  );
}

export function Dot({ tone, pulse = false }: { tone: Tone; pulse?: boolean }) {
  return <span aria-hidden className={cx("size-[7px] shrink-0 rounded-full", toneSolid[tone], pulse && "animate-live")} />;
}

export function Count({ children, tone }: { children: ReactNode; tone?: Tone }) {
  return (
    <span
      className={cx(
        "rounded-full px-2 py-px text-[11px] font-semibold",
        tone ? toneSoft[tone] : "bg-surface-3 text-muted",
      )}
    >
      {children}
    </span>
  );
}

export function Card({ className, children }: { className?: string; children: ReactNode }) {
  return <div className={cx("rounded-[14px] border border-line bg-surface shadow-card", className)}>{children}</div>;
}

export function LiveChip() {
  return (
    <span className="inline-flex items-center gap-1.5 rounded-full border border-work/25 px-2 py-[3px] text-[9px] font-bold tracking-[1.2px] text-work-text">
      <span className="size-1.5 rounded-full bg-work animate-live" aria-hidden />
      LIVE
    </span>
  );
}

export function PageIntro({
  eyebrow,
  title,
  subtitle,
  actions,
}: {
  eyebrow?: ReactNode;
  title: ReactNode;
  subtitle?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <header className="flex flex-wrap items-end justify-between gap-4">
      <div className="min-w-0">
        {eyebrow && <div className="mb-2">{eyebrow}</div>}
        <h1 className="text-[23px] leading-tight font-bold tracking-[-0.5px] text-ink">{title}</h1>
        {subtitle && <p className="mt-1.5 text-[12px] text-muted">{subtitle}</p>}
      </div>
      {actions && <div className="flex items-center gap-2">{actions}</div>}
    </header>
  );
}

export function SectionTitle({ children, count, tone }: { children: ReactNode; count?: number; tone?: Tone }) {
  return (
    <h2 className="flex items-center gap-2 text-[15px] font-bold text-ink">
      {children}
      {count != null && <Count tone={tone}>{count}</Count>}
    </h2>
  );
}

/** Small uppercase label, used above values and groups. */
export function Label({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <p className={cx("text-[10px] font-bold tracking-[0.8px] text-subtle uppercase", className)}>{children}</p>
  );
}

export function IconTile({ icon: Icon, tone, size = "md" }: { icon: LucideIcon; tone: Tone; size?: "md" | "lg" }) {
  return (
    <span
      className={cx(
        "grid shrink-0 place-items-center rounded-[9px]",
        size === "lg" ? "size-11 rounded-[12px]" : "size-8",
        toneSoft[tone],
      )}
    >
      <Icon size={size === "lg" ? 20 : 16} strokeWidth={2} aria-hidden />
    </span>
  );
}

export function Notice({
  tone,
  icon: Icon,
  children,
}: {
  tone: Tone;
  icon: LucideIcon;
  children: ReactNode;
}) {
  return (
    <div role={tone === "danger" ? "alert" : undefined} className={cx("flex gap-2 rounded-[10px] px-3 py-2.5 text-[12px] leading-relaxed", toneSoft[tone])}>
      <Icon size={14} strokeWidth={2.2} className="mt-0.5 shrink-0" aria-hidden />
      <div className="min-w-0">{children}</div>
    </div>
  );
}

export function EmptyState({
  icon,
  title,
  children,
}: {
  icon: LucideIcon;
  title: string;
  children?: ReactNode;
}) {
  return (
    <Card className="flex flex-col items-center px-8 py-12 text-center">
      <IconTile icon={icon} tone="accent" size="lg" />
      <h3 className="mt-4 text-[15px] font-bold text-ink">{title}</h3>
      {children}
    </Card>
  );
}

export function Code({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <code
      className={cx(
        "truncate rounded-[5px] bg-surface-3 px-1.5 py-px font-mono text-[10.5px] text-muted",
        className,
      )}
    >
      {children}
    </code>
  );
}
