import { expect, test } from "bun:test";
import { dashboardFixFailure, retainUnverifiedFindings, verifyDashboardToggleFix } from "./fixVerification";
import { getToggleById } from "@/registry";
import type { ScanFinding } from "@/components/startup/WizardAnimations";

const finding: ScanFinding = { id: "recallSnapshots", label: "Disable Recall", category: "privacy", severity: "warning", impact: "Policy is not configured" };
const receipt = { success: true, data: { status: "disabled", verified: true } };
const expectRejection = async (operation: Promise<void>, message: string) => {
  const result = await operation.then(() => "unexpected success", error => String(error));
  expect(result).toContain(message);
};

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
  await expectRejection(verifyDashboardToggleFix("officeLog", true, { ...wrapped, data: { ...wrapped.data, status: "blocked" } }, async () => ({ success: true, data: { officeLoggingDisabled: true } })), "not confirmed");
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
