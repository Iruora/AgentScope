import { useState } from "react";

import { useIntegrations, useSaveSettings, useSetHooks, useSettings } from "../lib/api";
import { harnessNames } from "../lib/format";
import type { Diff, HarnessId, HookStatus, Integration, Settings as SettingsData } from "../lib/types";

const statusLabels: Record<HookStatus, string> = {
  not_installed: "Not connected",
  installed: "Connected",
  needs_trust: "Waiting for trust",
  outdated: "Needs update",
};

export function Settings() {
  return (
    <div className="mx-auto max-w-3xl px-6 py-6">
      <h1 className="mb-6 text-xl font-semibold">Settings</h1>
      <IntegrationsSection />
      <NotificationsSection />
    </div>
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
    <section className="mb-10">
      <h2 className="text-sm font-semibold">Integrations</h2>
      <p className="mt-1 mb-3 text-sm text-zinc-500">
        Connecting adds observe-only hooks to the harness's user settings, so AgentScope sees
        sessions, tool calls, and approval prompts the moment they happen. The hooks run in the
        background, never answer prompts, and Disconnect removes exactly what was added. Without
        hooks, AgentScope still follows transcripts and running processes.
      </p>
      {shimMissing && (
        <p className="mb-3 rounded-lg bg-amber-50 p-3 text-sm text-amber-900 dark:bg-amber-950 dark:text-amber-200">
          This build does not include the agentscope-hook helper, so harnesses cannot be connected.
          Run <code>pnpm sidecar</code> and restart, or reinstall AgentScope.
        </p>
      )}
      {error && (
        <p role="alert" className="mb-3 rounded-lg bg-red-50 p-3 text-sm text-red-800 dark:bg-red-950 dark:text-red-300">
          {error}
        </p>
      )}
      <ul className="divide-y divide-zinc-200 rounded-xl border border-zinc-200 bg-white dark:divide-zinc-800 dark:border-zinc-800 dark:bg-zinc-900">
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
    </section>
  );
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
  let status: string;
  if (!detection.installed) status = "Not found on this machine";
  else if (!supported) status = "Coming soon";
  else status = statusLabels[hookStatus];

  return (
    <li className="flex items-center justify-between gap-4 px-4 py-3">
      <div>
        <p className="font-medium">{harnessNames[detection.harness]}</p>
        <p className="text-xs text-zinc-500">
          {status}
          {detection.configDir && supported ? ` · ${detection.configDir}` : ""}
        </p>
      </div>
      {detection.installed && supported && (
        <div className="flex gap-2">
          {hookStatus !== "installed" && (
            <button
              type="button"
              disabled={disabled}
              onClick={onConnect}
              className="rounded-md bg-zinc-900 px-3 py-1 text-sm font-medium text-white disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900"
            >
              {hookStatus === "outdated" ? "Update" : "Connect"}
            </button>
          )}
          {(connected || hookStatus === "outdated") && (
            <button
              type="button"
              disabled={disabled}
              onClick={onDisconnect}
              className="rounded-md border border-zinc-300 px-3 py-1 text-sm disabled:opacity-50 dark:border-zinc-700"
            >
              Disconnect
            </button>
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

  return (
    <div role="dialog" aria-modal="true" aria-labelledby="diff-title" className="fixed inset-0 z-10 flex items-center justify-center bg-black/40 p-6">
      <div className="flex max-h-full w-full max-w-3xl flex-col rounded-xl bg-white shadow-xl dark:bg-zinc-900">
        <header className="border-b border-zinc-200 p-4 dark:border-zinc-800">
          <h3 id="diff-title" className="font-semibold">
            {verb} {harnessNames[harness]}
          </h3>
          <p className="mt-1 text-sm text-zinc-500">
            {unchanged ? "Nothing to change in " : diff.before === null ? "This creates " : "This edits "}
            <code className="text-xs">{diff.path}</code>
            {!unchanged && diff.before !== null && ". The original is backed up next to it before the first edit."}
          </p>
        </header>
        {!unchanged && (
          <pre className="min-h-0 flex-1 overflow-auto p-4 text-xs leading-5">
            {lines.map((l, idx) => (
              <div
                key={idx}
                className={
                  l.sign === "+"
                    ? "bg-emerald-50 text-emerald-900 dark:bg-emerald-950 dark:text-emerald-200"
                    : l.sign === "-"
                      ? "bg-red-50 text-red-900 dark:bg-red-950 dark:text-red-200"
                      : "text-zinc-500"
                }
              >
                {l.sign} {l.text}
              </div>
            ))}
          </pre>
        )}
        <footer className="flex justify-end gap-2 border-t border-zinc-200 p-4 dark:border-zinc-800">
          <button type="button" onClick={onCancel} className="rounded-md border border-zinc-300 px-3 py-1 text-sm dark:border-zinc-700">
            Cancel
          </button>
          {!unchanged && (
            <button
              type="button"
              disabled={busy}
              onClick={onConfirm}
              className="rounded-md bg-zinc-900 px-3 py-1 text-sm font-medium text-white disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900"
            >
              {verb}
            </button>
          )}
        </footer>
      </div>
    </div>
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
    <section>
      <h2 className="mb-3 text-sm font-semibold">Notifications</h2>
      <ul className="divide-y divide-zinc-200 rounded-xl border border-zinc-200 bg-white dark:divide-zinc-800 dark:border-zinc-800 dark:bg-zinc-900">
        {options.map(([key, label, help]) => (
          <li key={key}>
            <label className="flex cursor-pointer items-center justify-between gap-4 px-4 py-3">
              <span>
                <span className="block text-sm font-medium">{label}</span>
                <span className="block text-xs text-zinc-500">{help}</span>
              </span>
              <input type="checkbox" className="size-4" checked={settings.data![key]} onChange={() => toggle(key)} />
            </label>
          </li>
        ))}
      </ul>
      {save.isError && <p className="mt-2 text-sm text-red-600">{String(save.error)}</p>}
    </section>
  );
}
