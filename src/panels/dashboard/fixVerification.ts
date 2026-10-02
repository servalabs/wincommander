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

export function assertDashboardFixSucceeded(result: unknown): void {
  if (!result || typeof result !== "object") return;
  const reply = result as Record<string, unknown>;
  const data = reply.data && typeof reply.data === "object"
    ? reply.data as Record<string, unknown> : undefined;
  const failed = [reply, data].some(value => value && (
    value.success === false || value.ok === false || !!value.error ||
    [value.status, value.operationStatus].some(status =>
      ["blocked", "failed", "error", "unsupported", "not_supported", "pending", "partial", "partially_applied"].includes(String(status)))
  ));
  if (!failed) return;
  const reason = [reply.error, data?.reason, data?.message, data?.error, data?.note, reply.reason, reply.message, reply.note]
    .find(value => typeof value === "string" && value.trim().length > 0);
  throw new Error(typeof reason === "string" ? reason : "Windows did not apply this change. The issue is still listed.");
}

export function requiresObservedFix(toggleId: string): boolean {
  return Object.hasOwn(DASHBOARD_POLICY_FIELDS, toggleId);
}

export async function verifyDependencyInstall(
  id: string,
  result: FixReply,
  readStatus: () => Promise<FixReply>,
): Promise<void> {
  const receipt = result.data as { success?: boolean; error?: boolean | string; message?: string } | null;
  if (!result.success || receipt?.success === false || receipt?.error) {
    throw new Error(result.error || receipt?.message || "Engine installation failed.");
  }
  const observed = await readStatus();
  if (!observed.success) throw new Error(observed.error || "The engine installation could not be checked.");
  const payload = observed.data as { dependencies?: { id: string; installed: boolean }[] } | null;
  if (!payload?.dependencies?.some((dependency) => dependency.id === id && dependency.installed === true)) {
    throw new Error("The required engine was not detected after installation. It has not been marked fixed. Refresh engines and retry.");
  }
}

/** A command acknowledgement is not proof that the Windows policy persisted. */
export async function verifyDashboardToggleFix(
  toggleId: string,
  targetChecked: boolean,
  result: FixReply,
  readHardeningStatus: () => Promise<FixReply>,
): Promise<void> {
  assertDashboardFixSucceeded(result);
  if (!requiresObservedFix(toggleId)) return;
  const receipt = result.data as { verified?: unknown; status?: unknown; operationStatus?: unknown; actuallyActive?: unknown } | null;
  const operationStatus = receipt?.operationStatus ?? receipt?.status;
  if (toggleId === "kernelDmaProtect") {
    if (receipt?.actuallyActive !== targetChecked) {
      throw new Error(receipt?.actuallyActive === false
        ? "Kernel DMA Protection is not active. Firmware/IOMMU support and possibly a restart are required. It has not been marked fixed."
        : receipt?.actuallyActive === true
          ? "Kernel DMA Protection is still active. Clearing its preference does not disable firmware protection. It has not been marked fixed."
          : "Windows could not confirm whether Kernel DMA Protection is active. It has not been marked fixed.");
    }
  } else if (receipt?.verified !== true || (receipt.status !== "applied" && receipt.status !== "disabled" && receipt.status !== "enabled") || operationStatus !== (targetChecked ? "disabled" : "enabled")) {
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
