import { Bell, CircleAlert, FileCode2, Plug, TriangleAlert } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { useEffect, useState } from "react";

import { Button, Card, Code, IconTile, Notice, PageIntro, Pill, cx, type Tone } from "../components/ui";
import { useIntegrations, useSaveSettings, useSetHooks, useSettings } from "../lib/api";
import { harnessNames } from "../lib/format";
import type { Diff, HarnessId, HookStatus, Integration, Settings as SettingsData } from "../lib/types";

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

function IntegrationsSection() {
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

function initials(name: string): string {
  return name
    .split(/\s+/)
    .map((w) => w[0])
    .join("")
    .slice(0, 2)
    .toUpperCase();
}

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
  const { detection, hookStatus, supported } = integration;
  const connected = hookStatus === "installed" || hookStatus === "needs_trust";
  const name = harnessNames[detection.harness];
  let status: ReactNode;
  if (!detection.installed) status = <Pill>Not found on this machine</Pill>;
  else if (!supported) status = <Pill tone="accent">Coming soon</Pill>;
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
          {initials(name)}
        </span>
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <p className="text-[13px] font-semibold text-ink">{name}</p>
            {status}
          </div>
          {detection.configDir && supported && (
            <Code className="mt-1 inline-block max-w-full">{detection.configDir}</Code>
          )}
        </div>
      </div>
      {detection.installed && supported && (
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

function NotificationsSection() {
  const settings = useSettings();
  const save = useSaveSettings();
  if (!settings.data) return null;
  const toggle = (key: keyof SettingsData) => save.mutate({ ...settings.data!, [key]: !settings.data![key] });

  const options: [keyof SettingsData, string, string][] = [
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
