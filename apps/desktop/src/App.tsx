import { useCallback, useState } from "react";

import { useGraphUpdates, useOpenSessionRequests, useSnapshot } from "./lib/api";
import { Approvals } from "./views/Approvals";
import { Overview } from "./views/Overview";
import { SessionDetail } from "./views/SessionDetail";
import { Settings } from "./views/Settings";

const sections = ["Overview", "Approvals", "History", "Settings"] as const;
type Section = (typeof sections)[number];

/** History arrives with step 4 of the delivery plan. */
const comingSoon: Section[] = ["History"];

type Route = { section: Section; sessionId?: string };

export function App() {
  useGraphUpdates();
  const snapshot = useSnapshot();
  const approvals = snapshot.data?.approvals.length ?? 0;
  const [route, setRoute] = useState<Route>({ section: "Overview" });

  const openSession = useCallback(
    (sessionId: string) => setRoute((r) => ({ section: r.section, sessionId })),
    [],
  );
  useOpenSessionRequests(openSession);

  let view;
  if (route.sessionId) {
    view = <SessionDetail sessionId={route.sessionId} onBack={() => setRoute({ section: route.section })} />;
  } else if (route.section === "Approvals") {
    view = <Approvals onOpen={openSession} />;
  } else if (route.section === "Settings") {
    view = <Settings />;
  } else {
    view = <Overview onOpen={openSession} />;
  }

  return (
    <div className="flex h-screen">
      <nav className="flex w-52 shrink-0 flex-col gap-1 border-r border-zinc-200 p-3 dark:border-zinc-800">
        <p className="px-3 py-2 text-sm font-semibold tracking-tight">AgentScope</p>
        {sections.map((name) => {
          const active = name === route.section && !route.sessionId;
          const disabled = comingSoon.includes(name);
          return (
            <button
              key={name}
              type="button"
              disabled={disabled}
              title={disabled ? "Coming soon" : undefined}
              aria-current={active ? "page" : undefined}
              onClick={() => setRoute({ section: name })}
              className={`flex items-center justify-between rounded-md px-3 py-1.5 text-left text-sm ${
                active
                  ? "bg-zinc-200/70 font-medium dark:bg-zinc-800"
                  : disabled
                    ? "text-zinc-400 dark:text-zinc-600"
                    : "hover:bg-zinc-100 dark:hover:bg-zinc-900"
              }`}
            >
              {name}
              {name === "Approvals" && approvals > 0 && (
                <span className="rounded-full bg-amber-500 px-1.5 text-xs text-white">{approvals}</span>
              )}
            </button>
          );
        })}
      </nav>
      <main className="flex-1 overflow-y-auto">{view}</main>
    </div>
  );
}
