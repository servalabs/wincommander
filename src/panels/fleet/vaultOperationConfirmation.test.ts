import { expect, test } from "bun:test";
import { newVaultEntry, type VaultAccessPolicy, type VaultAuthorizedEntry, type VaultMountEntryResult } from "./vaultAccessTypes";
import { recoverVaultPolicySave, vaultDraftConflictReason, vaultMountResultConfirmed, vaultPolicyRevisionConfirmed } from "./vaultOperationConfirmation";

const entry = { ...newVaultEntry(), id: "fixture", primary_owner_sid: "owner-a" };
const policy: VaultAccessPolicy = { schema_version: 1, policy_id: "policy", version: 2, expected_previous_version: 1, entries: [entry] };
const mounted: VaultMountEntryResult = { entry_id: entry.id, state: "mounted", drive_letter: "V", reason: null, presentation: "per-user" };
const observed: VaultAuthorizedEntry = { entry_id: entry.id, label: "Example", access: "write", presentation: "per-user", container_kind: "standard", mount_state: "mounted", drive_letter: "V:" };

test("mount success needs caller-authorized matching observed state and drive", () => {
  expect(vaultMountResultConfirmed(mounted, [observed])).toBe(true);
  expect(vaultMountResultConfirmed(mounted, [])).toBe(false);
  expect(vaultMountResultConfirmed(mounted, [{ ...observed, drive_letter: "W:" }])).toBe(false);
  expect(vaultMountResultConfirmed(mounted, [{ ...observed, mount_state: "unmounted", drive_letter: null }])).toBe(false);
  expect(vaultMountResultConfirmed({ ...mounted, drive_letter: null }, [{ ...observed, drive_letter: null }])).toBe(false);
  expect(vaultMountResultConfirmed(mounted, [{ ...observed, presentation: "machine" }])).toBe(false);
});

test("dismount success cannot be inferred from losing visibility of a vault", () => {
  const result = { ...mounted, state: "unmounted" as const, drive_letter: null };
  expect(vaultMountResultConfirmed(result, [observed])).toBe(false);
  expect(vaultMountResultConfirmed(result, [])).toBe(false);
  expect(vaultMountResultConfirmed(result, [{ ...observed, mount_state: "unmounted", drive_letter: null }])).toBe(true);
});

test("an old revision or still-present removed entry cannot show saved or deleted success", () => {
  expect(vaultPolicyRevisionConfirmed(policy, { ...policy, version: 1 }, [])).toBe(false);
  expect(vaultPolicyRevisionConfirmed(policy, policy, [])).toBe(true);
  expect(vaultPolicyRevisionConfirmed(policy, { ...policy, version: 3, entries: [{ ...entry, label: "Not saved" }] }, [])).toBe(false);
  expect(vaultPolicyRevisionConfirmed(policy, { ...policy, entries: [{ ...entry, primary_owner_sid: "owner-b" }] }, [])).toBe(false);
  expect(vaultPolicyRevisionConfirmed({ ...policy, entries: [] }, policy, [entry.id])).toBe(false);
  expect(vaultPolicyRevisionConfirmed({ ...policy, entries: [] }, null, [entry.id])).toBe(true);
});

test("save readback rejects a different encrypted file and changed Syncthing choice", () => {
  const expected = { ...policy, entries: [{ ...entry, container_path: "D:\\fixture\\one.hc", syncthing_opt_in: false }] };
  expect(vaultPolicyRevisionConfirmed(expected, { ...expected, entries: [{ ...expected.entries[0], container_path: "D:\\fixture\\two.hc" }] }, [])).toBe(false);
  expect(vaultPolicyRevisionConfirmed(expected, { ...expected, entries: [{ ...expected.entries[0], syncthing_opt_in: true }] }, [])).toBe(false);
  expect(vaultPolicyRevisionConfirmed(expected, { ...expected, entries: [{ ...expected.entries[0], container_path: "\\\\?\\D:\\fixture\\one.hc" }] }, [])).toBe(true);
});

test("an uncertain save uses bounded observations and never replays its mutation", async () => {
  let reads = 0;
  const recovered = await recoverVaultPolicySave(policy, [], async () => ++reads === 1 ? { ...policy, version: 1 } : policy, async () => {});
  expect(recovered.confirmed).toBe(true); expect(reads).toBe(2);
  reads = 0;
  const unknown = await recoverVaultPolicySave(policy, [], async () => { reads++; throw new Error("service unavailable"); }, async () => {});
  expect(unknown.confirmed).toBe(false); expect(reads).toBe(3);
});

test("policy confirmation compares all grant access while tolerating order and name case", () => {
  const expected = { ...policy, entries: [{ ...entry, grants: [
    { principal_name: "Owner", access: "write" as const }, { principal_name: "Reader", access: "read" as const },
  ] }] };
  const confirmed = { ...expected, entries: [{ ...expected.entries[0], grants: [
    { principal_name: " reader ", access: "read" as const }, { principal_name: "OWNER", access: "write" as const },
  ] }] };
  expect(vaultPolicyRevisionConfirmed(expected, confirmed, [])).toBe(true);
  expect(vaultPolicyRevisionConfirmed(expected, { ...confirmed, entries: [{ ...confirmed.entries[0], grants: [
    { principal_name: "Reader", access: "write" }, { principal_name: "Owner", access: "write" },
  ] }] }, [])).toBe(false);
  expect(vaultPolicyRevisionConfirmed(expected, { ...confirmed, entries: [{ ...confirmed.entries[0], grants: [
    { principal_name: "Intruder", access: "read" }, { principal_name: "Owner", access: "write" },
  ] }] }, [])).toBe(false);
});

test("legacy owners without SIDs are checked by name, SID owners tolerate display-name updates", () => {
  const legacy = { ...policy, entries: [{ ...entry, primary_owner_sid: null, owner_account: "Owner" }] };
  expect(vaultPolicyRevisionConfirmed(legacy, { ...legacy, entries: [{ ...legacy.entries[0], owner_account: "OWNER" }] }, [])).toBe(true);
  expect(vaultPolicyRevisionConfirmed(legacy, { ...legacy, entries: [{ ...legacy.entries[0], owner_account: "Other" }] }, [])).toBe(false);
  expect(vaultPolicyRevisionConfirmed(policy, { ...policy, entries: [{ ...entry, owner_account: "Renamed" }] }, [])).toBe(true);
});

test("a draft whose private owner changed reports ownership rather than stale version", () => {
  const transferred = { ...policy, entries: [{ ...entry, primary_owner_sid: "owner-b" }] };
  expect(vaultDraftConflictReason(policy, policy, transferred, "owner-a")).toBe("vault_owner_required");
  expect(vaultDraftConflictReason(policy, policy, policy, "owner-a")).toBe("Vault policy version conflict");
});

test("a missing projection never recovers an uncertain final removal", async () => {
  let reads = 0;
  const result = await recoverVaultPolicySave({ ...policy, entries: [] }, [entry.id], async () => { reads++; return null; }, async () => {});
  expect(result.confirmed).toBe(false);
  expect(reads).toBe(0);
});

test("uncertain partial removal cannot recover from caller visibility alone", async () => {
  const result = await recoverVaultPolicySave(policy, ["removed-entry"], async () => policy, async () => {});
  expect(result.confirmed).toBe(false);
});
