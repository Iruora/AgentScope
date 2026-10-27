import { useSnapshot } from "./lib/api";
import { Overview } from "./views/Overview";

const sections = ["Overview", "Approvals", "History", "Settings"] as const;

export function App() {
  const waiting = useSnapshot().data?.waitingCount ?? 0;

  return (
    <div className="flex h-screen">
      <nav className="flex w-52 shrink-0 flex-col gap-1 border-r border-zinc-200 p-3 dark:border-zinc-800">
        <p className="px-3 py-2 text-sm font-semibold tracking-tight">AgentScope</p>
        {sections.map((name) => {
          const active = name === "Overview";
          return (
            <button
              key={name}
              type="button"
              disabled={!active}
              aria-current={active ? "page" : undefined}
              className={`flex items-center justify-between rounded-md px-3 py-1.5 text-left text-sm ${
                active
                  ? "bg-zinc-200/70 font-medium dark:bg-zinc-800"
                  : "text-zinc-400 dark:text-zinc-600"
              }`}
            >
              {name}
              {name === "Approvals" && waiting > 0 && (
                <span className="rounded-full bg-amber-500 px-1.5 text-xs text-white">{waiting}</span>
              )}
            </button>
          );
        })}
      </nav>
      <main className="flex-1 overflow-y-auto">
        <Overview />
      </main>
    </div>
  );
}
