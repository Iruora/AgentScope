import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import { commands, events } from "../bindings";
import type { HarnessId, PushSettings, Settings, UpdateInfo } from "./types";

const snapshotKey = ["snapshot"] as const;
const answerableKey = ["answerable"] as const;

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
      void queryClient.invalidateQueries({ queryKey: answerableKey });
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
    mutationFn: (args: { harness: HarnessId; connect: boolean; dryRun: boolean; answerApprovals?: boolean }) =>
      unwrap(commands.setHooks(args.harness, args.connect, args.dryRun, args.answerApprovals ?? false)),
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

const updateKey = ["update"] as const;

/** The newer version found by the last check, if any. Background checks refresh it. */
export function useUpdate() {
  const queryClient = useQueryClient();
  useEffect(() => {
    const unlisten = events.updateAvailable.listen(() => {
      void queryClient.invalidateQueries({ queryKey: updateKey });
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [queryClient]);
  return useQuery({ queryKey: updateKey, queryFn: () => commands.getUpdate() });
}

/** Asks GitHub for a newer version now. */
export function useCheckForUpdate() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => unwrap(commands.checkForUpdate()),
    onSuccess: (update: UpdateInfo | null) => queryClient.setQueryData(updateKey, update),
  });
}

/** Installs the found update and restarts, or opens the download page for copies that cannot update themselves. */
export function useInstallUpdate() {
  return useMutation({
    mutationFn: (update: UpdateInfo) =>
      unwrap(update.canInstall ? commands.installUpdate() : commands.openReleasePage()),
  });
}

/** Settings → Diagnostics, refreshed while it is on screen. */
export function useDiagnostics() {
  return useQuery({
    queryKey: ["diagnostics"],
    queryFn: () => commands.getDiagnostics(),
    refetchInterval: 5000,
  });
}

/** Copies the redacted debug report to the clipboard. */
export function useCopyDebugReport() {
  return useMutation({
    mutationFn: () => unwrap(commands.copyDebugReport()),
  });
}

export function useOpenNewIssue() {
  return useMutation({ mutationFn: () => unwrap(commands.openNewIssue()) });
}

/** Ids of the approvals that can be answered in AgentScope right now. */
export function useAnswerable() {
  return useQuery({ queryKey: answerableKey, queryFn: () => commands.getAnswerable() });
}

/** Approves or denies a waiting prompt. */
export function useAnswerApproval() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (args: { approvalId: string; allow: boolean }) =>
      unwrap(commands.answerApproval(args.approvalId, args.allow)),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: answerableKey }),
  });
}

/** Hands a waiting prompt back to the agent's own terminal. */
export function useReleaseApproval() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (approvalId: string) => commands.releaseApproval(approvalId),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: answerableKey }),
  });
}

/** Shows the main window on a session. */
export function showSession(sessionId: string) {
  return commands.showSession(sessionId);
}

/** Closes the approval popup until a new prompt arrives. */
export function closePopup() {
  return commands.closePopup();
}

/** Sends a test push with settings that are not saved yet. */
export function useTestPush() {
  return useMutation({ mutationFn: (push: PushSettings) => unwrap(commands.testPush(push)) });
}
