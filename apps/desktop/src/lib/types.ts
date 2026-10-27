// The core only sends data to the UI, so the UI works with the serialized
// shape of each generated type.
export type {
  Activity_Serialize as Activity,
  Agent_Serialize as Agent,
  AgentState,
  AppInfo_Serialize as AppInfo,
  HarnessId,
  PendingApproval_Serialize as PendingApproval,
  Session_Serialize as Session,
  Snapshot_Serialize as Snapshot,
} from "../bindings";
