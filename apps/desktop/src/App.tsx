import { useQueryClient } from "@tanstack/react-query";
import { ArrowDownToLine, Bell, History, Inbox, LayoutGrid, Moon, Radar, RefreshCw, Settings as SettingsIcon, Sun } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useCallback, useState } from "react";

import { Button, Dot, IconButton, cx } from "./components/ui";
import { useGraphUpdates, useInstallUpdate, useOpenSessionRequests, useSettings, useSnapshot, useUpdate } from "./lib/api";
import { countByFilter, filterLabels, filterTones, liveSessions, type SessionFilter } from "./lib/filters";
import { useTheme } from "./lib/theme";
import { useNow } from "./lib/useNow";
import { Approvals } from "./views/Approvals";
import { History as HistoryView } from "./views/History";
import { Onboarding } from "./views/Onboarding";
import { Overview } from "./views/Overview";
import { SessionDetail } from "./views/SessionDetail";
import { Settings } from "./views/Settings";

type Section = "Overview" | "Approvals" | "History" | "Settings";

type Route = { section: Section; sessionId?: string; filter: SessionFilter };

export function App() {
  useGraphUpdates();
  const [route, setRoute] = useState<Route>({ section: "Overview", filter: "all" });

  const openSession = useCallback(
    (sessionId: string) => setRoute((r) => ({ ...r, sessionId })),
    [],
  );
  useOpenSessionRequests(openSession);
  const settings = useSettings();
  const go = (section: Section, filter: SessionFilter = "all") => setRoute({ section, filter });

  if (settings.isPending) return null;
  if (settings.data && !settings.data.onboarded) return <Onboarding brand={<Brand />} />;

  let view;
  if (route.sessionId) {
    view = <SessionDetail sessionId={route.sessionId} onBack={() => setRoute({ ...route, sessionId: undefined })} />;
  } else if (route.section === "Approvals") {
    view = <Approvals onOpen={openSession} />;
  } else if (route.section === "History") {
    view = <HistoryView onOpen={openSession} />;
  } else if (route.section === "Settings") {
    view = <Settings />;
  } else {
    view = (
      <Overview
        onOpen={openSession}
        filter={route.filter}
        onFilter={(filter) => setRoute({ section: "Overview", filter })}
        onSettings={() => go("Settings")}
      />
    );
  }

  return (
    <div className="grid h-screen grid-cols-[236px_minmax(0,1fr)]">
      <Sidebar route={route} go={go} />
      <div className="flex min-h-0 min-w-0 flex-col">
        <Topbar onApprovals={() => go("Approvals")} />
        <main className="min-h-0 flex-1 overflow-y-auto px-7 py-6">{view}</main>
      </div>
    </div>
  );
}

function Brand() {
  return (
    <div className="flex items-center gap-[9px] px-1.5 pb-1.5">
      <span className="grid size-8 place-items-center rounded-[9px] bg-linear-135 from-(--mark-from) to-(--mark-to) text-[#38bdf8] shadow-[0_6px_16px_rgb(8_145_178/0.28)]">
        <Radar size={18} strokeWidth={2.2} aria-hidden />
      </span>
      <span className="font-brand text-[19px] font-extrabold tracking-[-0.7px] text-ink">agentscope</span>
      <span aria-hidden className="mt-2 -ml-2 size-1.5 rounded-full bg-accent" />
    </div>
  );
}

function NavItem({
  icon: Icon,
  label,
  active,
  disabled,
  hint,
  count,
  countTone = "accent",
  onClick,
}: {
  icon: LucideIcon;
  label: string;
  active: boolean;
  disabled?: boolean;
  hint?: string;
  count?: number;
  countTone?: "accent" | "warn";
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      aria-current={active ? "page" : undefined}
      className={cx(
        "flex h-9 w-full items-center gap-2.5 rounded-[9px] px-2.5 text-left text-[13px] font-semibold transition-colors",
        active ? "bg-accent-soft text-accent-text" : "text-muted hover:enabled:bg-surface-2 hover:enabled:text-ink-2",
        disabled && "opacity-55",
      )}
    >
      <Icon size={16} strokeWidth={2} aria-hidden />
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {hint && (
        <span className="rounded-full bg-surface-3 px-1.5 py-px text-[9px] font-bold tracking-[0.6px] text-subtle uppercase">
          {hint}
        </span>
      )}
      {count != null && count > 0 && (
        <span
          className={cx(
            "min-w-[22px] rounded-full px-[7px] py-px text-center text-[10px] font-bold",
            countTone === "warn" ? "bg-warn text-[#2b1d00]" : "bg-accent text-white dark:text-[#04161d]",
          )}
        >
          {count}
        </span>
      )}
    </button>
  );
}

function Sidebar({ route, go }: { route: Route; go: (section: Section, filter?: SessionFilter) => void }) {
  const snapshot = useSnapshot();
  const live = liveSessions(snapshot.data?.sessions);
  const counts = countByFilter(live);
  const approvals = snapshot.data?.approvals.length ?? 0;
  const at = (section: Section) => route.section === section && !route.sessionId;

  return (
    <nav className="flex flex-col gap-[18px] overflow-x-hidden overflow-y-auto border-r border-line bg-surface px-3.5 py-5">
      <Brand />
      <div className="flex flex-col gap-0.5">
        <NavItem
          icon={LayoutGrid}
          label="Overview"
          active={at("Overview") && route.filter === "all"}
          count={live.length}
          onClick={() => go("Overview")}
        />
        <div className="my-0.5 mb-2 ml-[18px] flex flex-col gap-px border-l border-line-soft pl-2.5">
          {(Object.keys(filterLabels) as (keyof typeof filterLabels)[]).map((f) => (
            <button
              key={f}
              type="button"
              aria-current={at("Overview") && route.filter === f ? "page" : undefined}
              onClick={() => go("Overview", f)}
              className={cx(
                "flex h-[30px] items-center gap-2.5 rounded-[9px] px-2 text-left text-[12px] transition-colors",
                at("Overview") && route.filter === f
                  ? "bg-accent-soft font-semibold text-accent-text"
                  : "text-muted hover:bg-surface-2 hover:text-ink-2",
              )}
            >
              <Dot tone={filterTones[f]} />
              <span className="flex-1 truncate">{filterLabels[f]}</span>
              <span className="text-[11px] font-semibold text-subtle">{counts[f]}</span>
            </button>
          ))}
        </div>
        <NavItem
          icon={Inbox}
          label="Approvals"
          active={at("Approvals")}
          count={approvals}
          countTone="warn"
          onClick={() => go("Approvals")}
        />
        <NavItem icon={History} label="History" active={at("History")} onClick={() => go("History")} />
      </div>
      <div className="mt-auto flex flex-col gap-3">
        <UpdateBanner />
        <NavItem icon={SettingsIcon} label="Settings" active={at("Settings")} onClick={() => go("Settings")} />
      </div>
    </nav>
  );
}

function UpdateBanner() {
  const update = useUpdate();
  const install = useInstallUpdate();
  if (!update.data) return null;
  const { version, canInstall } = update.data;
  return (
    <div className="rounded-[10px] bg-accent-soft px-3 py-2.5">
      <p className="flex items-center gap-1.5 text-[12px] font-semibold text-accent-text">
        <ArrowDownToLine size={14} aria-hidden />
        Version {version} is available
      </p>
      <Button
        size="sm"
        variant="primary"
        className="mt-2 w-full"
        disabled={install.isPending}
        onClick={() => install.mutate(update.data!)}
      >
        {canInstall ? (install.isPending ? "Updating…" : "Update and restart") : "Download"}
      </Button>
      {install.isError && <p className="mt-1.5 text-[11px] text-danger-text">{install.error.message}</p>}
    </div>
  );
}

function syncedText(updatedAt: number, now: number): string {
  if (!updatedAt) return "Connecting…";
  const seconds = Math.floor((now - updatedAt) / 1000);
  if (seconds < 10) return "Updated just now";
  if (seconds < 60) return `Updated ${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `Updated ${minutes}m ago`;
  return `Updated ${Math.floor(minutes / 60)}h ago`;
}

function Topbar({ onApprovals }: { onApprovals: () => void }) {
  const queryClient = useQueryClient();
  const snapshot = useSnapshot();
  const now = useNow(5000);
  const [theme, toggleTheme] = useTheme();
  const approvals = snapshot.data?.approvals.length ?? 0;

  return (
    <header className="flex h-14 shrink-0 items-center justify-between gap-3 border-b border-line bg-surface px-7">
      <div className="flex items-center gap-2.5">
        <IconButton
          icon={RefreshCw}
          label="Refresh"
          onClick={() => void queryClient.invalidateQueries()}
          className={snapshot.isFetching ? "[&_svg]:animate-spin" : ""}
        />
        <span className="text-[11px] text-subtle">{syncedText(snapshot.dataUpdatedAt, now)}</span>
      </div>
      <div className="flex items-center gap-2.5">
        <IconButton
          icon={Bell}
          label={approvals ? `${approvals} waiting for approval` : "Approvals"}
          badge={approvals}
          onClick={onApprovals}
        />
        <IconButton
          icon={theme === "dark" ? Sun : Moon}
          label={theme === "dark" ? "Use light theme" : "Use dark theme"}
          onClick={toggleTheme}
        />
      </div>
    </header>
  );
}
