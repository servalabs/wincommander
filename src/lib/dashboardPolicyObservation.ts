import { getToggleById } from "@/registry";
import { setByPath } from "@/types/toggles";

export const DASHBOARD_POLICY_FIELDS: Record<string, string> = {
  recallSnapshots: "recallSnapshotsDisabled",
  internetComm: "internetCommRestricted",
  officeLog: "officeLoggingDisabled",
  bitlockerAuto: "bitlockerAutoEncryptDisabled",
};

/** Preserve explicit unknown observations instead of retaining an old cached success. */
export function preserveDashboardPolicyUnknowns(patch: Record<string, unknown>, hardening: object | undefined): Record<string, unknown> {
  if (!hardening) return patch;
  const values = hardening as Record<string, unknown>;
  for (const [toggleId, field] of Object.entries(DASHBOARD_POLICY_FIELDS)) {
    if (values[field] !== null) continue;
    const toggle = getToggleById(toggleId);
    if (toggle) setByPath(patch, toggle.currentPath, null);
  }
  return patch;
}
