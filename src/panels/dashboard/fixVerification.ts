import type { ScanFinding } from "@/components/startup/WizardAnimations";
import { DASHBOARD_POLICY_FIELDS } from "@/lib/dashboardPolicyObservation";

interface FixReply {
  success: boolean;
  error?: string;
  data?: unknown;
}

export interface FindingFixAttempt {
  finding: ScanFinding;
  error: string | null;
}

export function requiresObservedFix(toggleId: string): boolean {
  return Object.hasOwn(DASHBOARD_POLICY_FIELDS, toggleId);
}

/** A command acknowledgement is not proof that the Windows policy persisted. */
export async function verifyDashboardToggleFix(
  toggleId: string,
  targetChecked: boolean,
  result: FixReply,
  readHardeningStatus: () => Promise<FixReply>,
): Promise<void> {
  if (!result.success) throw new Error(result.error || "Windows did not apply this change. The issue is still listed.");
  if (!requiresObservedFix(toggleId)) return;
  const receipt = result.data as { verified?: unknown; status?: unknown; operationStatus?: unknown } | null;
  const operationStatus = receipt?.operationStatus ?? receipt?.status;
  if (receipt?.verified !== true || (receipt.status !== "applied" && receipt.status !== "disabled" && receipt.status !== "enabled") || operationStatus !== (targetChecked ? "disabled" : "enabled")) {
    throw new Error("Windows has not confirmed this change. The issue remains listed; refresh its status before retrying.");
  }
  const observed = await readHardeningStatus();
  const actual = observed.data as Record<string, unknown> | null;
  if (!observed.success || typeof actual?.[DASHBOARD_POLICY_FIELDS[toggleId]] !== "boolean") {
    throw new Error("The change was requested, but Windows could not be checked afterwards. It has not been marked fixed.");
  }
  if (actual[DASHBOARD_POLICY_FIELDS[toggleId]] !== targetChecked) {
    throw new Error("Windows still reports the previous setting after this change. A Windows or organization policy may be overriding it. The issue has not been marked fixed.");
  }
}

export function retainUnverifiedFindings(findings: readonly ScanFinding[], attempts: Record<string, FindingFixAttempt>): ScanFinding[] {
  const retained = Object.values(attempts).map(attempt => attempt.finding);
  const ids = new Set(retained.map(finding => finding.id));
  return [...retained, ...findings.filter(finding => !ids.has(finding.id))];
}

export function dashboardFixFailure(error: unknown): string {
  const text = error instanceof Error ? error.message : typeof error === "string" ? error : "The change could not be confirmed. The issue has not been marked fixed.";
  return text.replace(/\s+/g, " ").trim().slice(0, 360);
}
