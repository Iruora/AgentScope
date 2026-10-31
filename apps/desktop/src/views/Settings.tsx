import { Bell, CircleAlert, CircleCheck, ClipboardCopy, FileCode2, History, Plug, RefreshCw, Stethoscope, TriangleAlert } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { useEffect, useState } from "react";

import { Button, Card, Code, IconTile, Notice, PageIntro, Pill, cx, type Tone } from "../components/ui";
import {
  useAppInfo,
  useCheckForUpdate,
  useCopyDebugReport,
  useDiagnostics,
  useOpenNewIssue,
  useInstallUpdate,
  useIntegrations,
  useSaveSettings,
  useSetHooks,
  useSettings,
  useUpdate,
} from "../lib/api";
import { elapsed, harnessNames } from "../lib/format";
import { useNow } from "../lib/useNow";
import type { Diff, HarnessDiagnostics, HarnessId, HookStatus, Integration, Settings as SettingsData } from "../lib/types";

const statusLabels: Record<HookStatus, string> = {
  not_installed: "Not connected",
  installed: "Connected",
  needs_trust: "Waiting for trust",
  outdated: "Needs update",
};

const statusTones: Record<HookStatus, Tone> = {
  not_installed: "neutral",
  installed: "work",
  needs_trust: "warn",
  outdated: "warn",
};

export function Settings() {
  return (
    <div className="mx-auto flex max-w-[860px] flex-col gap-5">
      <PageIntro title="Settings" subtitle="Choose which harnesses AgentScope listens to, and when it should tap you on the shoulder." />
      <IntegrationsSection />
      <NotificationsSection />
      <HistorySection />
      <UpdatesSection />
      <DiagnosticsSection />
    </div>
  );
}

function SettingsCard({
  icon,
  title,
  description,
  children,
}: {
  icon: LucideIcon;
  title: string;
  description: ReactNode;
  children: ReactNode;
}) {
  return (
    <Card className="overflow-hidden">
      <header className="flex gap-3 border-b border-line-soft px-5 py-4">
        <IconTile icon={icon} tone="accent" />
        <div className="min-w-0">
          <h2 className="text-[14px] font-bold text-ink">{title}</h2>
          <p className="mt-1 text-[12px] leading-relaxed text-muted">{description}</p>
        </div>
      </header>
      {children}
    </Card>
  );
}

type Pending = { harness: HarnessId; connect: boolean; diff: Diff };

export function IntegrationsSection() {
  const integrations = useIntegrations();
  const setHooks = useSetHooks();
  const [pending, setPending] = useState<Pending | null>(null);
  const [error, setError] = useState<string | null>(null);

  const preview = async (harness: HarnessId, connect: boolean) => {
    setError(null);
    try {
      const diff = await setHooks.mutateAsync({ harness, connect, dryRun: true });
      setPending({ harness, connect, diff });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };
  const apply = async () => {
    if (!pending) return;
    try {
      await setHooks.mutateAsync({ harness: pending.harness, connect: pending.connect, dryRun: false });
      setPending(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const shimMissing = integrations.isSuccess && integrations.data.shimPath === null;

  return (
    <SettingsCard
      icon={Plug}
      title="Integrations"
      description="Connecting adds observe-only hooks to the harness's user settings, so AgentScope sees sessions, tool calls, and approval prompts the moment they happen. The hooks run in the background, never answer prompts, and Disconnect removes exactly what was added. Without hooks, AgentScope still follows transcripts and running processes."
    >
      {(shimMissing || error) && (
        <div className="flex flex-col gap-2 px-5 pt-4">
          {shimMissing && (
            <Notice tone="warn" icon={TriangleAlert}>
              This build does not include the agentscope-hook helper, so harnesses cannot be connected. Run{" "}
              <code className="font-mono">pnpm sidecar</code> and restart, or reinstall AgentScope.
            </Notice>
          )}
          {error && (
            <Notice tone="danger" icon={CircleAlert}>
              {error}
            </Notice>
          )}
        </div>
      )}
      <ul className="divide-y divide-line-soft">
        {(integrations.data?.harnesses ?? []).map((i) => (
          <IntegrationRow
            key={i.detection.harness}
            integration={i}
            disabled={shimMissing || setHooks.isPending}
            onConnect={() => preview(i.detection.harness, true)}
            onDisconnect={() => preview(i.detection.harness, false)}
          />
        ))}
      </ul>
      {pending && (
        <DiffDialog
          pending={pending}
          busy={setHooks.isPending}
          onCancel={() => setPending(null)}
          onConfirm={apply}
        />
      )}
    </SettingsCard>
  );
}

// Distinct marks for harnesses whose names share initials.
const harnessMarks: Record<HarnessId, string> = {
  "claude-code": "CC",
  codex: "CX",
  "copilot-cli": "GH",
  cursor: "CU",
};

function IntegrationRow({
  integration,
  disabled,
  onConnect,
  onDisconnect,
}: {
  integration: Integration;
  disabled: boolean;
  onConnect: () => void;
  onDisconnect: () => void;
}) {
  const { detection, hookStatus } = integration;
  const connected = hookStatus === "installed" || hookStatus === "needs_trust";
  const name = harnessNames[detection.harness];
  let status: ReactNode;
  if (!detection.installed) status = <Pill>Not found on this machine</Pill>;
  else status = <Pill tone={statusTones[hookStatus]}>{statusLabels[hookStatus]}</Pill>;

  return (
    <li className="flex items-center justify-between gap-4 px-5 py-3.5">
      <div className="flex min-w-0 items-center gap-3">
        <span
          className={cx(
            "grid size-9 shrink-0 place-items-center rounded-[10px] text-[11px] font-bold",
            detection.installed ? "bg-accent-soft text-accent-text" : "bg-surface-3 text-subtle",
          )}
        >
          {harnessMarks[detection.harness]}
        </span>
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <p className="text-[13px] font-semibold text-ink">{name}</p>
            {status}
          </div>
          {detection.configDir && <Code className="mt-1 inline-block max-w-full">{detection.configDir}</Code>}
          {hookStatus === "needs_trust" && (
            <p className="mt-1.5 text-[11px] text-warn-text">
              Codex runs new hooks only after you trust them: open Codex, type <code className="font-mono">/hooks</code>,
              and trust the AgentScope entries.
            </p>
          )}
        </div>
      </div>
      {detection.installed && (
        <div className="flex gap-2">
          {(connected || hookStatus === "outdated") && (
            <Button size="sm" disabled={disabled} onClick={onDisconnect}>
              Disconnect
            </Button>
          )}
          {hookStatus !== "installed" && (
            <Button size="sm" variant="primary" disabled={disabled} onClick={onConnect}>
              {hookStatus === "outdated" ? "Update" : "Connect"}
            </Button>
          )}
        </div>
      )}
    </li>
  );
}

/** A line diff of the config change, so nothing is written unseen. */
function diffLines(before: string, after: string): { sign: " " | "+" | "-"; text: string }[] {
  const a = before.split("\n");
  const b = after.split("\n");
  // Longest common subsequence; config files are small.
  const lcs = Array.from({ length: a.length + 1 }, () => new Array<number>(b.length + 1).fill(0));
  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) {
      lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
    }
  }
  const out: { sign: " " | "+" | "-"; text: string }[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length || j < b.length) {
    if (i < a.length && j < b.length && a[i] === b[j]) {
      out.push({ sign: " ", text: a[i++] });
      j++;
    } else if (j < b.length && (i === a.length || lcs[i][j + 1] >= lcs[i + 1][j])) {
      out.push({ sign: "+", text: b[j++] });
    } else {
      out.push({ sign: "-", text: a[i++] });
    }
  }
  return out;
}

function DiffDialog({
  pending,
  busy,
  onCancel,
  onConfirm,
}: {
  pending: Pending;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { diff, connect, harness } = pending;
  const unchanged = diff.before === diff.after;
  const lines = diffLines(diff.before ?? "", diff.after ?? "");
  const verb = connect ? "Connect" : "Disconnect";

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="diff-title"
      className="animate-fade fixed inset-0 z-50 grid place-items-center bg-(--scrim) p-6"
    >
      <div className="animate-pop flex max-h-full w-full max-w-3xl flex-col overflow-hidden rounded-2xl bg-surface shadow-pop">
        <header className="border-b border-line-soft px-5 pt-[18px] pb-3.5">
          <p className="mb-1 text-[11px] font-semibold text-muted">Review the change</p>
          <h3 id="diff-title" className="text-[17px] font-bold text-ink">
            {verb} {harnessNames[harness]}
          </h3>
          <p className="mt-1.5 flex flex-wrap items-center gap-1 text-[12px] text-muted">
            <FileCode2 size={13} aria-hidden />
            {unchanged ? "Nothing to change in " : diff.before === null ? "This creates " : "This edits "}
            <Code>{diff.path}</Code>
            {!unchanged && diff.before !== null && ". The original is backed up next to it before the first edit."}
          </p>
        </header>
        {!unchanged && (
          <pre className="m-5 min-h-0 flex-1 overflow-auto rounded-[10px] border border-line-soft bg-surface-2 py-2 font-mono text-[11px] leading-5">
            {lines.map((l, idx) => (
              <div
                key={idx}
                className={cx(
                  "px-3",
                  l.sign === "+" ? "bg-work-soft text-work-text" : l.sign === "-" ? "bg-danger-soft text-danger-text" : "text-muted",
                )}
              >
                {l.sign} {l.text}
              </div>
            ))}
          </pre>
        )}
        <footer className="flex justify-end gap-2 border-t border-line-soft px-5 py-3.5">
          <Button onClick={onCancel}>Cancel</Button>
          {!unchanged && (
            <Button variant="primary" disabled={busy} onClick={onConfirm}>
              {verb}
            </Button>
          )}
        </footer>
      </div>
    </div>
  );
}

function Switch({ checked, onChange, label }: { checked: boolean; onChange: () => void; label: string }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={onChange}
      className={cx(
        "relative h-5 w-9 shrink-0 rounded-full transition-colors",
        checked ? "bg-accent" : "bg-surface-3 ring-1 ring-line ring-inset",
      )}
    >
      <span
        aria-hidden
        className={cx(
          "absolute top-0.5 left-0.5 size-4 rounded-full bg-white shadow-sm transition-transform",
          checked && "translate-x-4",
        )}
      />
    </button>
  );
}

type NotifyKey = "notifyApprovals" | "notifyInput";

function NotificationsSection() {
  const settings = useSettings();
  const save = useSaveSettings();
  if (!settings.data) return null;
  const toggle = (key: NotifyKey) => save.mutate({ ...settings.data!, [key]: !settings.data![key] });

  const options: [NotifyKey, string, string][] = [
    ["notifyApprovals", "An agent needs approval", "A permission prompt is waiting in a terminal."],
    ["notifyInput", "An agent is waiting for input", "It finished its turn or asked you a question."],
  ];
  return (
    <SettingsCard icon={Bell} title="Notifications" description="Desktop notifications for the moments an agent is blocked on you.">
      <ul className="divide-y divide-line-soft">
        {options.map(([key, label, help]) => (
          <li key={key} className="flex items-center justify-between gap-4 px-5 py-3.5">
            <span>
              <span className="block text-[13px] font-semibold text-ink">{label}</span>
              <span className="mt-0.5 block text-[11px] text-muted">{help}</span>
            </span>
            <Switch checked={settings.data![key]} onChange={() => toggle(key)} label={label} />
          </li>
        ))}
      </ul>
      {save.isError && (
        <div className="px-5 pb-4">
          <Notice tone="danger" icon={CircleAlert}>
            {String(save.error)}
          </Notice>
        </div>
      )}
    </SettingsCard>
  );
}

const RETENTION_CHOICES = [1, 3, 7, 14, 30, 90];

function HistorySection() {
  const settings = useSettings();
  const save = useSaveSettings();
  if (!settings.data) return null;
  const current: SettingsData = settings.data;
  return (
    <SettingsCard
      icon={History}
      title="History"
      description="Ended sessions and their events stay on this machine for this long, then are deleted. Live sessions are always kept."
    >
      <div className="flex items-center justify-between gap-4 px-5 py-3.5">
        <label htmlFor="retention" className="text-[13px] font-semibold text-ink">
          Keep history for
        </label>
        <select
          id="retention"
          value={current.retentionDays}
          disabled={save.isPending}
          onChange={(e) => save.mutate({ ...current, retentionDays: Number(e.target.value) })}
          className="h-8 rounded-[9px] border border-line bg-surface px-2.5 text-[12px] font-semibold text-ink outline-none focus:border-accent focus:shadow-[0_0_0_3px_var(--accent-ring)]"
        >
          {RETENTION_CHOICES.map((days) => (
            <option key={days} value={days}>
              {days} day{days === 1 ? "" : "s"}
            </option>
          ))}
        </select>
      </div>
      {save.isError && (
        <div className="px-5 pb-4">
          <Notice tone="danger" icon={CircleAlert}>
            {String(save.error)}
          </Notice>
        </div>
      )}
    </SettingsCard>
  );
}

function UpdatesSection() {
  const settings = useSettings();
  const save = useSaveSettings();
  const appInfo = useAppInfo();
  const update = useUpdate();
  const check = useCheckForUpdate();
  const install = useInstallUpdate();
  if (!settings.data) return null;
  const current: SettingsData = settings.data;
  const found = update.data;
  const error = check.error ?? install.error ?? save.error;

  return (
    <SettingsCard
      icon={RefreshCw}
      title="Updates"
      description="AgentScope checks GitHub for a new release when it starts and every few hours. Updates are signed, and nothing installs until you choose to."
    >
      <ul className="divide-y divide-line-soft">
        <li className="flex items-center justify-between gap-4 px-5 py-3.5">
          <span>
            <span className="block text-[13px] font-semibold text-ink">Check for updates automatically</span>
            <span className="mt-0.5 block text-[11px] text-muted">You get a notification when a new version is out.</span>
          </span>
          <Switch
            checked={current.checkUpdates}
            onChange={() => save.mutate({ ...current, checkUpdates: !current.checkUpdates })}
            label="Check for updates automatically"
          />
        </li>
        <li className="flex items-center justify-between gap-4 px-5 py-3.5">
          <span>
            <span className="block text-[13px] font-semibold text-ink">
              {found ? `Version ${found.version} is available` : `Version ${appInfo.data?.version ?? ""}`}
            </span>
            <span className="mt-0.5 flex items-center gap-1 text-[11px] text-muted">
              {found && appInfo.data && `You have version ${appInfo.data.version}.`}
              {!found && !check.isSuccess && "The version you are running."}
              {!found && check.isSuccess && (
                <>
                  <CircleCheck size={12} className="text-work-text" aria-hidden /> It is up to date.
                </>
              )}
            </span>
          </span>
          {found ? (
            <Button size="sm" variant="primary" disabled={install.isPending} onClick={() => install.mutate(found)}>
              {found.canInstall ? (install.isPending ? "Updating…" : "Update and restart") : "Download"}
            </Button>
          ) : (
            <Button size="sm" disabled={check.isPending} onClick={() => check.mutate()}>
              {check.isPending ? "Checking…" : "Check now"}
            </Button>
          )}
        </li>
      </ul>
      {error && (
        <div className="px-5 pb-4">
          <Notice tone="danger" icon={CircleAlert}>
            {error.message}
          </Notice>
        </div>
      )}
    </SettingsCard>
  );
}

/** True when the detected version is outside the release line the adapter was tested with. */
export function untestedVersion(h: HarnessDiagnostics): boolean {
  if (!h.version || !/^\d/.test(h.testedVersion)) return false;
  const version = h.version.match(/\d+(\.\d+)*/)?.[0] ?? "";
  return version !== h.testedVersion && !version.startsWith(`${h.testedVersion}.`);
}

function DiagnosticsSection() {
  const diagnostics = useDiagnostics();
  const copy = useCopyDebugReport();
  const openIssue = useOpenNewIssue();
  const now = useNow();
  const d = diagnostics.data;
  if (!d) return null;
  const error = copy.error ?? openIssue.error;

  return (
    <SettingsCard
      icon={Stethoscope}
      title="Diagnostics"
      description="What each harness has sent since AgentScope started. If something looks wrong, copy the debug report into a GitHub issue. It hides your home folder and anything that looks like a secret."
    >
      <ul className="divide-y divide-line-soft">
        {d.harnesses
          .filter((h) => h.installed || h.hooksReceived > 0 || h.transcriptEvents > 0)
          .map((h) => (
            <li key={h.harness} className="px-5 py-3.5">
              <div className="flex flex-wrap items-center gap-2">
                <p className="text-[13px] font-semibold text-ink">{harnessNames[h.harness]}</p>
                <Pill>{h.version ? `v${h.version}` : "version unknown"}</Pill>
                {untestedVersion(h) && (
                  <Pill tone="warn" icon={TriangleAlert}>
                    Tested with {h.testedVersion}.x
                  </Pill>
                )}
                <Pill tone={statusTones[h.hookStatus]}>{statusLabels[h.hookStatus]}</Pill>
              </div>
              <dl className="mt-2 grid grid-cols-[repeat(auto-fit,minmax(150px,1fr))] gap-x-4 gap-y-1 text-[11px]">
                <Stat label="Hook events">
                  {h.hooksReceived}
                  {h.unhandledHooks > 0 && <span className="text-warn-text"> ({h.unhandledHooks} not understood)</span>}
                </Stat>
                <Stat label="Last hook">
                  {h.lastHookAt ? `${elapsed(h.lastHookAt, now)} ago · ${h.lastHookEvent ?? ""}` : "none yet"}
                </Stat>
                <Stat label="Transcript events">{h.transcriptEvents}</Stat>
                <Stat label="Live sessions">{h.liveSessions}</Stat>
              </dl>
              {h.hookStatus === "installed" && h.hooksReceived === 0 && h.liveSessions > 0 && (
                <p className="mt-1.5 text-[11px] text-warn-text">
                  Connected, but no hook has arrived while a session is live. Restart that session so it loads the hooks.
                </p>
              )}
            </li>
          ))}
        <li className="flex flex-wrap items-center justify-between gap-3 px-5 py-3.5">
          <span className="text-[11px] text-muted">
            Listening on {d.ingestPort ? `127.0.0.1:${d.ingestPort}` : "nothing (hooks are being spooled)"}
            {" · "}
            {d.spoolFiles > 0 ? `${d.spoolFiles} spooled file(s) waiting` : "spool empty"}
            {!d.shimPath && " · hook helper missing"}
            {d.recentLogs.length > 0 && ` · ${d.recentLogs.length} recent warning(s)`}
          </span>
          <span className="flex gap-2">
            <Button size="sm" icon={ClipboardCopy} disabled={copy.isPending} onClick={() => copy.mutate()}>
              {copy.isSuccess ? "Copied" : "Copy debug report"}
            </Button>
            <Button size="sm" onClick={() => openIssue.mutate()}>
              Report a problem
            </Button>
          </span>
        </li>
      </ul>
      {error && (
        <div className="px-5 pb-4">
          <Notice tone="danger" icon={CircleAlert}>
            {error.message}
          </Notice>
        </div>
      )}
    </SettingsCard>
  );
}

function Stat({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex gap-1.5">
      <dt className="text-subtle">{label}</dt>
      <dd className="font-semibold text-ink">{children}</dd>
    </div>
  );
}
