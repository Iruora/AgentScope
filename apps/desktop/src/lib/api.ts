import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import { commands, events } from "../bindings";

const snapshotKey = ["snapshot"] as const;

/** The live graph. Refetched whenever the core reports a change. */
export function useSnapshot() {
  const queryClient = useQueryClient();
  useEffect(() => {
    const unlisten = events.graphChanged.listen(() => {
      void queryClient.invalidateQueries({ queryKey: snapshotKey });
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [queryClient]);
  return useQuery({ queryKey: snapshotKey, queryFn: () => commands.getSnapshot() });
}

export function useAppInfo() {
  return useQuery({ queryKey: ["appInfo"], queryFn: () => commands.getAppInfo() });
}
