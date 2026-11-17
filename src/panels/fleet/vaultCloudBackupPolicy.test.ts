// SPDX-License-Identifier: AGPL-3.0-or-later
import { expect, test } from "bun:test";
import { newVaultEntry, newVaultPolicy, normalizeVaultAccessPolicy, vaultOwnerFragmentFromPolicy } from "./vaultAccessTypes";
import { applyVaultAccessPreset } from "./vaultAccessPresets";
import { vaultAccessEntryIntentEqual } from "./vaultAccessDraft";

test("cloud backup defaults denied and explicit permission survives policy submission", () => {
  const entry = newVaultEntry();
  expect(entry.cloud_backup).toBeNull();
  const changed = { ...entry, cloud_backup: "owner_only" as const };
  expect(vaultAccessEntryIntentEqual(entry, changed)).toBe(false);
  const policy = { ...newVaultPolicy(), entries: [changed] };
  expect(vaultOwnerFragmentFromPolicy(policy).entries[0]!.entry.cloud_backup).toBe("owner_only");
  const { cloud_backup: _omitted, ...legacy } = entry;
  expect(normalizeVaultAccessPolicy({ ...policy, entries: [legacy] }).entries[0]!.cloud_backup).toBeNull();
});
test("switching to a personal Vault removes write-member cloud permission", () => {
  const entry = { ...newVaultEntry("shared"), cloud_backup: "write_members" as const };
  expect(applyVaultAccessPreset(entry, "private").cloud_backup).toBe("owner_only");
  expect(applyVaultAccessPreset({ ...entry, cloud_backup: null }, "private").cloud_backup).toBeNull();
});
