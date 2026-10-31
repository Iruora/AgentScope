import { Bell, CircleAlert, Smartphone, CircleCheck, ClipboardCopy, FileCode2, History, Keyboard, Plug, RefreshCw, ShieldCheck, Stethoscope, TriangleAlert } from "lucide-react";
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
  useTestPush,
  useUpdate,
} from "../lib/api";
import { elapsed, harnessNames } from "../lib/format";
import { useNow } from "../lib/useNow";
import { isMac, modKey } from "../components/CommandPalette";
import type { Diff, HarnessDiagnostics, HarnessId, HookStatus, Integration, PushService, PushSettings, Settings as SettingsData } from "../lib/types";

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
      <AnsweringSection />
      <PushSection />
      <ShortcutsSection />
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

type Pending = { harness: HarnessId; connect: boolean; answer: boolean; title: string; diff: Diff };

export function IntegrationsSection() {
  const integrations = useIntegrations();
  const setHooks = useSetHooks();
  const [pending, setPending] = useState<Pending | null>(null);
  const [error, setError] = useState<string | null>(null);

  const preview = async (harness: HarnessId, connect: boolean, answer: boolean, title: string) => {
    setError(null);
    try {
      const diff = await setHooks.mutateAsync({ harness, connect, dryRun: true, answerApprovals: answer });
      setPending({ harness, connect, answer, title, diff });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };
  const apply = async () => {
    if (!pending) return;
    try {
      await setHooks.mutateAsync({
        harness: pending.harness,
        connect: pending.connect,
        dryRun: false,
        answerApprovals: pending.answer,
      });
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
      description="Connecting adds observe-only hooks to the harness's user settings, so AgentScope sees sessions, tool calls, and approval prompts the moment they happen. The hooks run in the background and Disconnect removes exactly what was added. They never answer prompts unless you turn on answering for a harness below. Without hooks, AgentScope still follows transcripts and running processes."
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
            onConnect={() => preview(i.detection.harness, true, i.answers, `Connect ${harnessNames[i.detection.harness]}`)}
            onDisconnect={() => preview(i.detection.harness, false, false, `Disconnect ${harnessNames[i.detection.harness]}`)}
            onToggleAnswer={() =>
              preview(
                i.detection.harness,
                true,
                !i.answers,
                i.answers
                  ? `Stop answering ${harnessNames[i.detection.harness]} approvals here`
                  : `Answer ${harnessNames[i.detection.harness]} approvals from AgentScope`,
              )
            }
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
  onToggleAnswer,
}: {
  integration: Integration;
  disabled: boolean;
  onConnect: () => void;
  onDisconnect: () => void;
  onToggleAnswer: () => void;
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
          {integration.canAnswer && hookStatus === "installed" && (
            <label className="mt-2 flex items-center gap-2.5 text-[12px] text-ink-2">
              <Switch
                checked={integration.answers}
                onChange={disabled ? () => {} : onToggleAnswer}
                label="Answer approvals from AgentScope"
              />
              <span>
                <span className="font-semibold">Answer approvals from AgentScope</span>
                <span className="block text-[11px] text-muted">
                  Approve or deny prompts in the Approvals inbox. Without an answer, the terminal asks as usual.
                </span>
              </span>
            </label>
          )}
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
  const { diff, connect, title } = pending;
  const unchanged = diff.before === diff.after;
  const lines = diffLines(diff.before ?? "", diff.after ?? "");
  const verb = connect ? (title.startsWith("Connect") ? "Connect" : "Apply") : "Disconnect";

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
            {title}
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

/** Must match `hotkey::CHOICES` in the Rust app. */
const SHORTCUT_CHOICES = ["CommandOrControl+Shift+Y", "CommandOrControl+Alt+A", "CommandOrControl+Shift+Period"];

/** "CommandOrControl+Shift+Y" → "⌘ ⇧ Y" on macOS, "Ctrl+Shift+Y" elsewhere. */
export function shortcutLabel(accelerator: string, mac = isMac): string {
  const names: Record<string, [string, string]> = {
    CommandOrControl: ["⌘", "Ctrl"],
    Shift: ["⇧", "Shift"],
    Alt: ["⌥", "Alt"],
    Period: [".", "."],
  };
  const parts = accelerator.split("+").map((p) => names[p]?.[mac ? 0 : 1] ?? p);
  return parts.join(mac ? " " : "+");
}

function ShortcutsSection() {
  const settings = useSettings();
  const save = useSaveSettings();
  if (!settings.data) return null;
  const current: SettingsData = settings.data;
  return (
    <SettingsCard
      icon={Keyboard}
      title="Shortcuts"
      description={
        <>
          Press <kbd className="font-semibold">{modKey} K</kbd> in AgentScope to search sessions, views and actions. A global
          shortcut can also take you to the agent that has waited longest for you, from any app (on Linux this needs an X11 session).
        </>
      }
    >
      <div className="flex items-center justify-between gap-4 px-5 py-3.5">
        <label htmlFor="global-shortcut" className="text-[13px] font-semibold text-ink">
          Go to the waiting agent
          <span className="mt-0.5 block text-[11px] font-normal text-muted">
            Focuses its terminal or editor. With nobody waiting, it opens AgentScope.
          </span>
        </label>
        <select
          id="global-shortcut"
          value={current.globalShortcut}
          disabled={save.isPending}
          onChange={(e) => save.mutate({ ...current, globalShortcut: e.target.value })}
          className="h-8 rounded-[9px] border border-line bg-surface px-2.5 text-[12px] font-semibold text-ink outline-none focus:border-accent focus:shadow-[0_0_0_3px_var(--accent-ring)]"
        >
          <option value="">Off</option>
          {SHORTCUT_CHOICES.map((choice) => (
            <option key={choice} value={choice}>
              {shortcutLabel(choice)}
            </option>
          ))}
        </select>
      </div>
      {save.isError && (
        <div className="px-5 pb-4">
          <Notice tone="danger" icon={CircleAlert}>
            {String(save.error.message)}
          </Notice>
        </div>
      )}
    </SettingsCard>
  );
}

/** Must match `DECIDE_WINDOWS` in the Rust settings. */
const DECIDE_WINDOWS = [15, 30, 60, 120];

function AnsweringSection() {
  const settings = useSettings();
  const save = useSaveSettings();
  const integrations = useIntegrations();
  if (!settings.data) return null;
  const current: SettingsData = settings.data;
  const answering = (integrations.data?.harnesses ?? []).filter((i) => i.answers);
  return (
    <SettingsCard
      icon={ShieldCheck}
      title="Answering approvals"
      description={
        answering.length
          ? `On for ${answering.map((i) => harnessNames[i.detection.harness]).join(", ")}. Prompts wait in the Approvals inbox for your answer, then go back to the terminal.`
          : "Off. Turn it on per harness under Integrations. Only Claude Code supports it so far."
      }
    >
      <ul className="divide-y divide-line-soft">
        <li className="flex items-center justify-between gap-4 px-5 py-3.5">
          <label htmlFor="decide-window" className="text-[13px] font-semibold text-ink">
            Wait for an answer here for
            <span className="mt-0.5 block text-[11px] font-normal text-muted">
              Then the agent asks in its terminal instead. Ask in terminal hands a prompt back at once.
            </span>
          </label>
          <select
            id="decide-window"
            value={current.decideWindowSecs}
            disabled={save.isPending}
            onChange={(e) => save.mutate({ ...current, decideWindowSecs: Number(e.target.value) })}
            className="h-8 rounded-[9px] border border-line bg-surface px-2.5 text-[12px] font-semibold text-ink outline-none focus:border-accent focus:shadow-[0_0_0_3px_var(--accent-ring)]"
          >
            {DECIDE_WINDOWS.map((secs) => (
              <option key={secs} value={secs}>
                {secs < 60 ? `${secs} seconds` : `${secs / 60} minute${secs === 60 ? "" : "s"}`}
              </option>
            ))}
          </select>
        </li>
        <li className="flex items-center justify-between gap-4 px-5 py-3.5">
          <span>
            <span className="block text-[13px] font-semibold text-ink">Confirm risky commands</span>
            <span className="mt-0.5 block text-[11px] text-muted">
              Approving a command like rm -rf, a force push or a piped install takes a second click.
            </span>
          </span>
          <Switch
            checked={current.confirmRisky}
            onChange={() => save.mutate({ ...current, confirmRisky: !current.confirmRisky })}
            label="Confirm risky commands"
          />
        </li>
        <li className="flex items-center justify-between gap-4 px-5 py-3.5">
          <span>
            <span className="block text-[13px] font-semibold text-ink">Pop up a small window</span>
            <span className="mt-0.5 block text-[11px] text-muted">
              A prompt you can answer here opens a small window in the corner of the screen. It never takes focus from what you are typing.
            </span>
          </span>
          <Switch
            checked={current.popupApprovals}
            onChange={() => save.mutate({ ...current, popupApprovals: !current.popupApprovals })}
            label="Pop up a small window"
          />
        </li>
      </ul>
      {save.isError && (
        <div className="px-5 pb-4">
          <Notice tone="danger" icon={CircleAlert}>
            {String(save.error.message)}
          </Notice>
        </div>
      )}
    </SettingsCard>
  );
}

const PUSH_SERVICES: { id: PushService; name: string; target?: string; targetHint?: string; token?: string; tokenHint?: string; help: string }[] = [
  { id: "off", name: "Off", help: "" },
  {
    id: "ntfy",
    name: "ntfy",
    target: "Topic",
    targetHint: "agentscope-7f3k2, or https://your-server/topic",
    token: "Access token (optional)",
    help: "Install the ntfy app and subscribe to a topic name that is hard to guess. Anyone who knows the topic can read it.",
  },
  {
    id: "pushover",
    name: "Pushover",
    target: "User key",
    token: "App token",
    help: "Create an application at pushover.net for its app token; your user key is on the dashboard.",
  },
  { id: "discord", name: "Discord webhook", target: "Webhook URL", targetHint: "https://discord.com/api/webhooks/…", help: "Channel settings → Integrations → Webhooks → New webhook → Copy URL." },
  { id: "slack", name: "Slack webhook", target: "Webhook URL", targetHint: "https://hooks.slack.com/services/…", help: "Create a Slack app with an incoming webhook and copy its URL." },
  {
    id: "telegram",
    name: "Telegram",
    target: "Chat id",
    token: "Bot token",
    help: "Create a bot with @BotFather for its token, send it a message, then use your chat id.",
  },
];

const inputClass =
  "h-8 w-full rounded-[9px] border border-line bg-surface px-2.5 text-[12px] text-ink outline-none placeholder:text-subtle focus:border-accent focus:shadow-[0_0_0_3px_var(--accent-ring)]";

function PushSection() {
  const settings = useSettings();
  const save = useSaveSettings();
  const test = useTestPush();
  const [draft, setDraft] = useState<PushSettings | null>(null);
  useEffect(() => {
    if (settings.data && draft === null) setDraft(settings.data.push);
  }, [settings.data, draft]);
  if (!settings.data || !draft) return null;
  const current: SettingsData = settings.data;
  const service = PUSH_SERVICES.find((s) => s.id === draft.service) ?? PUSH_SERVICES[0];
  const dirty = JSON.stringify(draft) !== JSON.stringify(current.push);
  const update = (patch: Partial<PushSettings>) => {
    test.reset();
    setDraft({ ...draft, ...patch });
  };
  const commit = (push: PushSettings) => save.mutate({ ...current, push });

  return (
    <SettingsCard
      icon={Smartphone}
      title="Phone push"
      description="Get a push on your phone when an agent needs you, through a service you choose. Off by default. When on, messages go through that service; AgentScope sends nothing else."
    >
      <ul className="divide-y divide-line-soft">
        <li className="flex items-center justify-between gap-4 px-5 py-3.5">
          <label htmlFor="push-service" className="text-[13px] font-semibold text-ink">
            Send through
          </label>
          <select
            id="push-service"
            value={draft.service}
            disabled={save.isPending}
            onChange={(e) => {
              const next = { ...draft, service: e.target.value as PushService };
              update(next);
              if (next.service === "off") commit(next);
            }}
            className="h-8 rounded-[9px] border border-line bg-surface px-2.5 text-[12px] font-semibold text-ink outline-none focus:border-accent focus:shadow-[0_0_0_3px_var(--accent-ring)]"
          >
            {PUSH_SERVICES.map((s) => (
              <option key={s.id} value={s.id}>
                {s.name}
              </option>
            ))}
          </select>
        </li>
        {service.id !== "off" && (
          <>
            <li className="flex flex-col gap-3 px-5 py-3.5">
              <p className="text-[11px] text-muted">{service.help}</p>
              <div className={cx("grid gap-3", service.token && "sm:grid-cols-2")}>
                <label className="flex flex-col gap-1 text-[11px] font-semibold text-muted">
                  {service.target}
                  <input
                    className={inputClass}
                    value={draft.target}
                    placeholder={service.targetHint}
                    spellCheck={false}
                    autoComplete="off"
                    onChange={(e) => update({ target: e.target.value })}
                  />
                </label>
                {service.token && (
                  <label className="flex flex-col gap-1 text-[11px] font-semibold text-muted">
                    {service.token}
                    <input
                      className={inputClass}
                      type="password"
                      value={draft.token}
                      autoComplete="off"
                      onChange={(e) => update({ token: e.target.value })}
                    />
                  </label>
                )}
              </div>
              <div className="flex flex-wrap items-center justify-end gap-2">
                {test.isSuccess && (
                  <span className="mr-auto inline-flex items-center gap-1 text-[11px] font-semibold text-work-text">
                    <CircleCheck size={13} aria-hidden /> Sent. Check your phone.
                  </span>
                )}
                <Button size="sm" disabled={test.isPending} onClick={() => test.mutate(draft)}>
                  {test.isPending ? "Sending…" : "Send a test"}
                </Button>
                <Button size="sm" variant="primary" disabled={!dirty || save.isPending} onClick={() => commit(draft)}>
                  Save
                </Button>
              </div>
            </li>
            <li className="flex items-center justify-between gap-4 px-5 py-3.5">
              <span>
                <span className="block text-[13px] font-semibold text-ink">Include names and commands</span>
                <span className="mt-0.5 block text-[11px] text-muted">
                  Off sends only "An agent needs your approval", so nothing about your code passes through the service.
                </span>
              </span>
              <Switch
                checked={draft.includeDetails}
                onChange={() => {
                  const next = { ...draft, includeDetails: !draft.includeDetails };
                  update(next);
                  if (!dirty) commit(next);
                }}
                label="Include names and commands"
              />
            </li>
            <li className="flex items-center justify-between gap-4 px-5 py-3.5">
              <span>
                <span className="block text-[13px] font-semibold text-ink">Also when an agent waits for input</span>
                <span className="mt-0.5 block text-[11px] text-muted">Otherwise only permission prompts send a push.</span>
              </span>
              <Switch
                checked={draft.includeInput}
                onChange={() => {
                  const next = { ...draft, includeInput: !draft.includeInput };
                  update(next);
                  if (!dirty) commit(next);
                }}
                label="Also when an agent waits for input"
              />
            </li>
          </>
        )}
      </ul>
      {(save.isError || test.isError) && (
        <div className="px-5 pb-4">
          <Notice tone="danger" icon={CircleAlert}>
            {String((test.error ?? save.error)?.message)}
          </Notice>
        </div>
      )}
    </SettingsCard>
  );
}
