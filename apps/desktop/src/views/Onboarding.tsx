import { ArrowRight, CircleAlert, Eye, HardDrive, SquareTerminal } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

import { Button, IconTile, Notice } from "../components/ui";
import { useIntegrations, useSaveSettings, useSettings } from "../lib/api";
import { IntegrationsSection } from "./Settings";

const points: [LucideIcon, string, string][] = [
  [Eye, "Observe only", "Hooks report what agents do. They never answer prompts or change what an agent runs."],
  [HardDrive, "Stays on this machine", "Everything is read and stored locally. AgentScope makes no network calls."],
  [SquareTerminal, "Answer where you work", "When an agent needs you, Go to agent brings its terminal or editor to the front."],
];

/** First-run welcome: what AgentScope does, and connecting the harnesses it found. */
export function Onboarding({ brand }: { brand: ReactNode }) {
  const settings = useSettings();
  const integrations = useIntegrations();
  const save = useSaveSettings();
  const finish = () => settings.data && save.mutate({ ...settings.data, onboarded: true });

  const found = (integrations.data?.harnesses ?? []).filter((i) => i.detection.installed).length;
  const connected = (integrations.data?.harnesses ?? []).filter(
    (i) => i.hookStatus === "installed" || i.hookStatus === "needs_trust",
  ).length;

  return (
    <main className="h-screen overflow-y-auto bg-bg px-7 py-10">
      <div className="mx-auto flex max-w-[860px] flex-col gap-7">
        {brand}
        <header>
          <h1 className="text-[28px] leading-tight font-bold tracking-[-0.6px] text-ink">
            Every agent. Every action. One view.
          </h1>
          <p className="mt-2 max-w-[620px] text-[13px] leading-relaxed text-muted">
            AgentScope shows the AI coding agents running on this machine, their subagents, what each one is doing,
            and which ones are waiting for your approval.
          </p>
        </header>

        <ul className="grid gap-3 sm:grid-cols-3">
          {points.map(([icon, title, body]) => (
            <li key={title} className="flex flex-col gap-2 rounded-[14px] border border-line bg-surface p-4 shadow-card">
              <IconTile icon={icon} tone="accent" />
              <span className="text-[13px] font-semibold text-ink">{title}</span>
              <span className="text-[11.5px] leading-relaxed text-muted">{body}</span>
            </li>
          ))}
        </ul>

        <section className="flex flex-col gap-3">
          <div>
            <h2 className="text-[15px] font-bold text-ink">Connect your harnesses</h2>
            <p className="mt-1 text-[12px] text-muted">
              {integrations.isSuccess
                ? found === 0
                  ? "No supported harness was found yet. You can connect one later in Settings."
                  : `Found ${found} on this machine. Connect each one to see approval prompts the moment they appear.`
                : "Looking for Claude Code, Codex, Copilot CLI and Cursor…"}
            </p>
          </div>
          <IntegrationsSection />
          <p className="text-[11px] text-subtle">
            Not ready to connect? AgentScope still follows transcripts and running processes, just without live
            approval prompts.
          </p>
        </section>

        {save.isError && (
          <Notice tone="danger" icon={CircleAlert}>
            {String(save.error)}
          </Notice>
        )}

        <footer className="flex items-center justify-end gap-2 border-t border-line pt-5">
          {connected === 0 && (
            <Button disabled={save.isPending} onClick={finish}>
              Skip for now
            </Button>
          )}
          <Button variant="primary" icon={ArrowRight} disabled={save.isPending || connected === 0} onClick={finish}>
            Start watching
          </Button>
        </footer>
      </div>
    </main>
  );
}
