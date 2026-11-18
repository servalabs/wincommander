// SPDX-License-Identifier: AGPL-3.0-or-later
import { expect, test } from "bun:test";
import { readVaultLetterCleanup, vaultLetterReportMessage } from "./vaultLetterCleanup";
const stale = { letter: "Q", category: "stale_wincommander_reservation" as const, released: true, saved_entry: false };
test("cleanup success requires a matching bounded ownership classification", () => {
  expect(readVaultLetterCleanup({ released: 1, letters: [stale] }).released).toBe(1);
  expect(readVaultLetterCleanup({ released: 0, letters: [{ ...stale, released: false, category: "system_or_policy_reserved_letter", vault_policy_reservation: true }] }).letters[0].vault_policy_reservation).toBe(true);
  for (const result of [{ released: 1 }, { released: 0, letters: [stale] }, { released: 1, letters: [{ ...stale, letter: "C" }] },
    { released: 1, letters: [{ ...stale, saved_entry: true }] }, { released: 1, letters: [{ ...stale, category: "external_or_unknown_owner" }] },
    { released: 0, letters: [{ ...stale, released: false, vault_policy_reservation: true }] }]) {
    let error = ""; try { readVaultLetterCleanup(result); } catch (failure) { error = (failure as Error).message; }
    expect(error).toBe("vault_drive_letter_cleanup_unverified");
  }
});
test("unreleasable letters explain their safe next action without raw service data", () => {
  expect(vaultLetterReportMessage({ ...stale, letter: "F", released: false, category: "real_windows_volume" })).toContain("Windows volume or device");
  expect(vaultLetterReportMessage({ ...stale, letter: "A", released: false, category: "system_or_policy_reserved_letter" })).toContain("D:–Z: range");
  expect(vaultLetterReportMessage({ ...stale, released: false, category: "external_or_unknown_owner" })).toContain("left it unchanged");
  expect(vaultLetterReportMessage({ ...stale, released: false, category: "external_or_unknown_owner", windows_mapping: "current_session_network" })).toContain("network drive mapped in your Windows session");
  expect(vaultLetterReportMessage({ ...stale, released: false, category: "system_or_policy_reserved_letter", vault_policy_reservation: true })).toContain("saved Vault policy");
  expect(vaultLetterReportMessage({ ...stale, released: false, category: "duplicate_saved_entry" })).toContain("duplicate");
});

test("describes verified Windows mappings and Vault reservations without guessing an owner", () => {
  const external = { ...stale, released: false, category: "external_or_unknown_owner" as const };
  const reserved = { ...stale, released: false, category: "system_or_policy_reserved_letter" as const, vault_policy_reservation: true };
  expect(vaultLetterReportMessage({ ...external, windows_mapping: "other_account" })).toContain("mapped by another Windows account");
  expect(vaultLetterReportMessage({ ...external, windows_mapping: "other_session" })).toContain("another session for your Windows account");
  expect(vaultLetterReportMessage({ ...reserved, vault_policy_owner_is_caller: false })).toContain("reserved for another Windows account");
  const report = readVaultLetterCleanup({ released: 0, letters: [{ ...reserved, vault_policy_owner_is_caller: false, vault_policy_owner: "Lab User" }] }).letters[0];
  expect(vaultLetterReportMessage(report)).toContain("reserved for Lab User by a saved Vault policy");
  for (const invalid of [{ ...external, vault_policy_owner: "Lab User" }, { ...reserved, windows_mapping: "other_account" }]) {
    let error = "";
    try { readVaultLetterCleanup({ released: 0, letters: [invalid] }); } catch (failure) { error = (failure as Error).message; }
    expect(error).toBe("vault_drive_letter_cleanup_unverified");
  }
});
