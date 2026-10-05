import { expect, test } from "bun:test";
import { assertDashboardFixSucceeded, dashboardFixFailure, retainUnverifiedFindings, verifyDashboardToggleFix, verifyDependencyInstall } from "./fixVerification";
import { getToggleById } from "@/registry";
import type { ScanFinding } from "@/components/startup/WizardAnimations";

const finding: ScanFinding = { id: "recallSnapshots", label: "Disable Recall", category: "privacy", severity: "warning", impact: "Policy is not configured" };
const receipt = { success: true, data: { status: "disabled", verified: true } };
const expectRejection = async (operation: Promise<void>, message: string) => {
  const result = await operation.then(() => "unexpected success", error => String(error));
  expect(result).toContain(message);
};

test("Fix All preserves backend reasons instead of confirming failed or unsupported scope", async () => {
  for (const result of [
    { success: false, error: "Administrator approval is required." },
    { success: true, data: { status: "blocked", reason: "Administrator approval is required." } },
    { success: true, data: { status: "failed", message: "Administrator approval is required." } },
    { success: true, data: { success: false, message: "Administrator approval is required." } },
    { success: true, data: { ok: false, message: "Administrator approval is required." } },
    { success: true, data: { error: true, message: "Administrator approval is required." } },
    { success: true, data: { status: "applied", operationStatus: "blocked", reason: "Administrator approval is required." } },
  ]) {
    await expectRejection(Promise.resolve().then(() => assertDashboardFixSucceeded(result)), "Administrator approval is required.");
  }
  await expectRejection(Promise.resolve().then(() => assertDashboardFixSucceeded({ success: true, data: { status: "unsupported", reason: "This device does not support this setting." } })), "This device does not support this setting.");
});

test("Fix All accepts successful current-user and machine receipts without changing scope or retrying", () => {
  for (const scope of ["current-user", "machine", "machine-and-current-user", "application-defined"]) {
    const result = { success: true, data: { scope, status: "applied" } };
    expect(assertDashboardFixSucceeded(result)).toBeUndefined();
    expect(result.data.scope).toBe(scope);
  }
  expect(assertDashboardFixSucceeded(undefined)).toBeUndefined();
});

test("pending and partial receipts retain their explanation instead of becoming success", async () => {
  for (const status of ["pending", "partial", "partially_applied"]) {
    await expectRejection(Promise.resolve().then(() => assertDashboardFixSucceeded({
      success: true, data: { status, note: "Protection is not active. Check firmware support and restart." },
    })), "Protection is not active. Check firmware support and restart.");
  }
  await expectRejection(verifyDashboardToggleFix("kernelDmaProtect", true,
    { success: true, data: { status: "pending", operationStatus: "preference_set_reboot_needed", actuallyActive: false, note: "Kernel DMA Protection is not active." } },
    async () => { throw new Error("must not probe"); }), "Kernel DMA Protection is not active.");
});

test("user-only toggle failure remains visible even when no hardening probe applies", async () => {
  await expectRejection(verifyDashboardToggleFix("clockSeconds", true,
    { success: true, data: { scope: "current-user", status: "failed", reason: "Windows denied the preference write." } },
    async () => { throw new Error("must not probe"); }), "Windows denied the preference write.");
});

test("DMA policy preference is not a confirmed hardware protection", async () => {
  let probes = 0;
  for (const scope of [undefined, "machine"]) {
    await expectRejection(verifyDashboardToggleFix("kernelDmaProtect", true,
      { success: true, data: { scope, status: scope ? "applied" : "preference_set_reboot_needed", operationStatus: "preference_set_reboot_needed", actuallyActive: false } },
      async () => { probes += 1; return { success: true, data: { kernelDmaProtect: true } }; }), "not active");
  }
  expect(probes).toBe(0);
  const active = { success: true, data: { status: "enabled", actuallyActive: true } };
  await expectRejection(verifyDashboardToggleFix("kernelDmaProtect", true, active,
    async () => ({ success: true, data: { kernelDmaProtect: false } })), "previous setting");
  expect(await verifyDashboardToggleFix("kernelDmaProtect", true, active,
    async () => ({ success: true, data: { kernelDmaProtect: true } }))).toBeUndefined();
  await expectRejection(verifyDashboardToggleFix("kernelDmaProtect", false,
    { success: true, data: { status: "preference_cleared", actuallyActive: true } },
    async () => { throw new Error("must not probe"); }), "still active");
  await expectRejection(verifyDashboardToggleFix("kernelDmaProtect", true,
    { success: true, data: { status: "applied" } },
    async () => { throw new Error("must not probe"); }), "could not confirm");
});

test("engine install acknowledgements require a fresh exact dependency readback", async () => {
  await expectRejection(verifyDependencyInstall("instantSearch", { success: true }, async () => ({ success: true, data: { dependencies: [{ id: "instantSearch", installed: false }] } })), "not detected");
  await expectRejection(verifyDependencyInstall("instantSearch", { success: true, data: { success: false, message: "Download failed" } }, async () => { throw new Error("must not probe"); }), "Download failed");
  await expectRejection(verifyDependencyInstall("instantSearch", { success: true }, async () => ({ success: false, error: "Status unavailable" })), "Status unavailable");
  expect(await verifyDependencyInstall("instantSearch", { success: true }, async () => ({ success: true, data: { dependencies: [{ id: "instantSearch", installed: true }] } }))).toBeUndefined();
});

test("an acknowledgement cannot remove an issue without verified receipt and independent readback", async () => {
  let probes = 0;
  await expectRejection(verifyDashboardToggleFix("recallSnapshots", true, { success: true }, async () => {
    probes += 1;
    return { success: true, data: { recallSnapshotsDisabled: true } };
  }), "not confirmed");
  expect(probes).toBe(0);
  await expectRejection(verifyDashboardToggleFix("recallSnapshots", true, receipt, async () => ({ success: true, data: { recallSnapshotsDisabled: false } })), "still reports the previous setting");
});

test("each of the four fixes requires its own exact observed boolean", async () => {
  for (const [id, key] of Object.entries({ recallSnapshots: "recallSnapshotsDisabled", internetComm: "internetCommRestricted", officeLog: "officeLoggingDisabled", bitlockerAuto: "bitlockerAutoEncryptDisabled" })) {
    expect(await verifyDashboardToggleFix(id, true, receipt, async () => ({ success: true, data: { [key]: true } }))).toBe(undefined);
    await expectRejection(verifyDashboardToggleFix(id, true, receipt, async () => ({ success: true, data: { [key]: "true" } })), "not been marked fixed");
    await expectRejection(verifyDashboardToggleFix(id, true, receipt, async () => ({ success: false })), "not been marked fixed");
  }
});

test("reverting a policy requires enabled receipt and false observed state", async () => {
  expect(await verifyDashboardToggleFix("internetComm", false, { success: true, data: { status: "enabled", verified: true } }, async () => ({ success: true, data: { internetCommRestricted: false } }))).toBe(undefined);
  await expectRejection(verifyDashboardToggleFix("internetComm", false, receipt, async () => ({ success: true, data: { internetCommRestricted: false } })), "not confirmed");
});

test("machine-wide receipt preserves the operation result and still needs independent readback", async () => {
  const wrapped = { success: true, data: { status: "applied", operationStatus: "disabled", verified: true } };
  expect(await verifyDashboardToggleFix("officeLog", true, wrapped, async () => ({ success: true, data: { officeLoggingDisabled: true } }))).toBe(undefined);
  await expectRejection(verifyDashboardToggleFix("officeLog", true, { ...wrapped, data: { ...wrapped.data, status: "blocked" } }, async () => ({ success: true, data: { officeLoggingDisabled: true } })), "did not apply");
});

test("failed native writes retain their reason and do not probe for a success", async () => {
  let probes = 0;
  await expectRejection(verifyDashboardToggleFix("officeLog", true, { success: false, error: "Windows denied this write" }, async () => {
    probes += 1;
    return { success: true };
  }), "Windows denied this write");
  expect(probes).toBe(0);
});

test("pending or failed rows survive optimistic cached snapshots without inventing new findings", () => {
  expect(retainUnverifiedFindings([], { recallSnapshots: { finding, error: null } })).toEqual([finding]);
  expect(retainUnverifiedFindings([], { recallSnapshots: { finding, error: "Not verified" } })).toEqual([finding]);
  expect(retainUnverifiedFindings([finding], { recallSnapshots: { finding, error: null } })).toEqual([finding]);
  expect(retainUnverifiedFindings([], {})).toEqual([]);
});

test("BitLocker wording describes automatic policy only and preserves existing action", () => {
  const toggle = getToggleById("bitlockerAuto")!;
  expect(toggle.enableCmd).toBe("Disable-BitLockerAutoEncrypt");
  expect(toggle.safeDefault).toBe(true);
  expect(toggle.description).toContain("does not encrypt or decrypt existing drives");
  expect(toggle.impact).not.toContain("Drive is not encrypted");
});

test("visible failure messages are bounded and support native string errors", () => {
  expect(dashboardFixFailure("  Windows\n denied   access ")).toBe("Windows denied access");
  expect(dashboardFixFailure(new Error("a".repeat(500)))).toHaveLength(360);
});

test("action notifications omit PowerShell details and boolean-only failures", () => {
  const raw = 'Administrator privileges required. Command: Disable-DiagnosticEventTracing At line:10 char:9 CategoryInfo : OperationStopped FullyQualifiedErrorId : Administrator';
  expect(dashboardFixFailure(raw)).toBe("Administrator approval is required. Open WinCommander as administrator and retry.");
  expect(dashboardFixFailure("Windows denied this write. Command: Disable-Test At line:1 char:1")).toBe("Windows denied this write.");
  expect(dashboardFixFailure("true")).toBe("The change could not be confirmed. Refresh its status and retry.");
});
