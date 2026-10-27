import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import { commands, events } from "../bindings";
import type { HarnessId, Settings } from "./types";

const snapshotKey = ["snapshot"] as const;

/** Unwraps a command result, turning its error into a thrown `Error`. */
async function unwrap<T>(
  result: Promise<{ status: "ok"; data: T } | { status: "error"; error: string }>,
): Promise<T> {
  const r = await result;
  if (r.status === "error") throw new Error(r.error);
  return r.data;
}

/** Refetches everything derived from the graph whenever the core reports a change. */
export function useGraphUpdates() {
  const queryClient = useQueryClient();
  useEffect(() => {
    const unlisten = events.graphChanged.listen(() => {
      void queryClient.invalidateQueries({ queryKey: snapshotKey });
      void queryClient.invalidateQueries({ queryKey: ["sessionEvents"] });
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [queryClient]);
}

/** The live graph. */
export function useSnapshot() {
  return useQuery({ queryKey: snapshotKey, queryFn: () => commands.getSnapshot() });
}

export function useAppInfo() {
  return useQuery({ queryKey: ["appInfo"], queryFn: () => commands.getAppInfo() });
}

export function useSessionEvents(sessionId: string) {
  return useQuery({
    queryKey: ["sessionEvents", sessionId],
    queryFn: () => unwrap(commands.getSessionEvents(sessionId)),
  });
}

export function useIntegrations() {
  return useQuery({ queryKey: ["integrations"], queryFn: () => commands.getIntegrations() });
}

/** Previews (dry run) or applies a connect / disconnect. */
export function useSetHooks() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (args: { harness: HarnessId; connect: boolean; dryRun: boolean }) =>
      unwrap(commands.setHooks(args.harness, args.connect, args.dryRun)),
    onSuccess: (_diff, args) => {
      if (!args.dryRun) void queryClient.invalidateQueries({ queryKey: ["integrations"] });
    },
  });
}

export function useSettings() {
  return useQuery({ queryKey: ["settings"], queryFn: () => commands.getSettings() });
}

export function useSaveSettings() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (settings: Settings) => unwrap(commands.setSettings(settings)),
    onSuccess: (_, settings) => {
      queryClient.setQueryData(["settings"], settings);
      // A shorter retention drops history from the graph.
      void queryClient.invalidateQueries({ queryKey: snapshotKey });
    },
  });
}

export function useGoToAgent() {
  return useMutation({
    mutationFn: (sessionId: string) => unwrap(commands.goToAgent(sessionId)),
  });
}

export function revealTranscript(sessionId: string) {
  return unwrap(commands.revealTranscript(sessionId));
}

/** Calls `open` with a session id when the tray asks to show one. */
export function useOpenSessionRequests(open: (sessionId: string) => void) {
  useEffect(() => {
    const unlisten = events.openSession.listen((e) => open(e.payload.sessionId));
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [open]);
}
