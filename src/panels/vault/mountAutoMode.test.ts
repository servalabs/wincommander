import { expect, test } from "bun:test";
import { mountPasswordSelectedVolume } from "./mountAutoMode";

const baseRequest = {
  volumePath: "C:\\Vaults\\vault.hc",
  driveLetter: "V",
  password: "test-password",
  readOnly: true,
  scope: "per-user" as const,
  hardenAcl: true,
};

test("normal mount chooses hidden mode only after a read-only password rejection", async () => {
  const calls: Array<{ volumeKind?: string; volumeRole?: string; readOnly?: boolean }> = [];
  const mounted = await mountPasswordSelectedVolume(async (request) => {
    calls.push(request);
    return calls.length === 1
      ? { success: false, error: "vault_engine_unlock_failed" }
      : { success: true, data: { status: "mounted", drive: "V", scope: "per-user", readOnly: true, removable: false, hiddenProtection: false, aclAttested: true } };
  }, baseRequest);

  expect(mounted.success).toBe(true);
  expect(calls.map(({ volumeKind, volumeRole, readOnly }) => ({ volumeKind, volumeRole, readOnly }))).toEqual([
    { volumeKind: "standard", volumeRole: "standard", readOnly: true },
    { volumeKind: "dual", volumeRole: "hidden", readOnly: true },
  ]);
});

test("normal mount does not retry hidden mode for a non-credential failure", async () => {
  const calls: Array<{ volumeKind?: string; volumeRole?: string }> = [];
  const result = await mountPasswordSelectedVolume(async (request) => {
    calls.push(request);
    return { success: false, error: "vault_driver_unavailable" };
  }, baseRequest);

  expect(result).toEqual({ success: false, error: "vault_driver_unavailable" });
  expect(calls.map(({ volumeKind, volumeRole }) => ({ volumeKind, volumeRole }))).toEqual([
    { volumeKind: "standard", volumeRole: "standard" },
  ]);
});
