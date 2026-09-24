import { expect, test } from "bun:test";
import { mountPasswordSelectedVolume } from "./mountAutoMode";

const baseRequest = {
  volumePath: "C:\\Vaults\\vault.hc",
  driveLetter: "V",
  password: "test-password",
  readOnly: false,
  scope: "per-user" as const,
  hardenAcl: true,
};

test("normal writable mount retains its access mode when falling back to hidden mode", async () => {
  const calls: Array<{ volumeKind?: string; volumeRole?: string; readOnly?: boolean }> = [];
  const mounted = await mountPasswordSelectedVolume(async (request) => {
    calls.push(request);
    return calls.length === 1
      ? { success: false, error: "vault_engine_unlock_failed" }
      : { success: true, data: { status: "mounted", drive: "V", scope: "per-user", readOnly: false, removable: false, hiddenProtection: false, aclAttested: true } };
  }, baseRequest);

  expect(mounted.success).toBe(true);
  expect(calls.map(({ volumeKind, volumeRole, readOnly }) => ({ volumeKind, volumeRole, readOnly }))).toEqual([
    { volumeKind: "standard", volumeRole: "standard", readOnly: false },
    { volumeKind: "dual", volumeRole: "hidden", readOnly: false },
  ]);
});

test("an explicit read-only request remains read-only for every attempt", async () => {
  const calls: Array<{ volumeKind?: string; volumeRole?: string; readOnly?: boolean }> = [];
  await mountPasswordSelectedVolume(async (request) => {
    calls.push(request);
    return calls.length === 1
      ? { success: false, error: "vault_engine_unlock_failed" }
      : { success: true, data: { status: "mounted", drive: "V", scope: "per-user", readOnly: true, removable: false, hiddenProtection: false, aclAttested: true } };
  }, { ...baseRequest, readOnly: true });

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

test("explicit current-account ACL repair consent survives standard-to-hidden detection", async () => {
  const calls: Array<{ volumeKind?: string; volumeRole?: string; repairCurrentAccountAccess?: boolean }> = [];
  await mountPasswordSelectedVolume(async (request) => {
    calls.push(request);
    return calls.length === 1
      ? { success: false, error: "vault_engine_unlock_failed" }
      : { success: false, error: "vault_caller_access_denied" };
  }, { ...baseRequest, repairCurrentAccountAccess: true });

  expect(calls.map(({ volumeKind, volumeRole, repairCurrentAccountAccess }) => ({
    volumeKind,
    volumeRole,
    repairCurrentAccountAccess,
  }))).toEqual([
    { volumeKind: "standard", volumeRole: "standard", repairCurrentAccountAccess: true },
    { volumeKind: "dual", volumeRole: "hidden", repairCurrentAccountAccess: true },
  ]);
});
