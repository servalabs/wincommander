import { invoke } from "@tauri-apps/api/core";

export const fleetRecovery = {
  pinOperatorKey: (operatorKey: string) => invoke<{ pinned: boolean }>("fleet_recovery_pin_operator_key", { operatorKey }),
  stage: (retirementReceipt: unknown) => invoke<{ candidate_public_key: string; submission_conflict?: boolean }>("fleet_recovery_stage", { retirementReceipt }),
  resume: () => invoke<{ status: string }>("fleet_recovery_resume"),
  handoff: (retirementReceipt: unknown) => invoke<unknown>("fleet_recovery_handoff", { retirementReceipt }),
  finish: (retirementReceipt: unknown, handoff: unknown) => invoke<{ status: string }>("fleet_recovery_finish", { retirementReceipt, handoff }),
};
