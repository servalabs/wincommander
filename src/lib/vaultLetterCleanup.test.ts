// SPDX-License-Identifier: AGPL-3.0-or-later
import { expect, test } from "bun:test";
import { freedLettersSummary, readVaultLetterCleanup, vaultLetterReportMessage, type VaultLetterHolder } from "./vaultLetterCleanup";
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

const king: VaultLetterHolder = { kind: "vault_mount", scope: "session", logon_state: "disconnected", device_present: false, owner_is_caller: false, owner: "SERVAULT\\King" };
const ramDisk: VaultLetterHolder = { kind: "ram_disk", scope: "machine", logon_state: "machine", device_present: true };
const freedT = { letter: "T", category: "stale_drive_mapping" as const, released: true, saved_entry: false, holder: king };
const external = { letter: "T", category: "external_or_unknown_owner" as const, released: false, saved_entry: false };

test("a freed dead link names the closed vault it was left over from", () => {
  const cleanup = readVaultLetterCleanup({ released: 1, letters: [freedT, { ...external, letter: "R", holder: ramDisk }] });
  expect(freedLettersSummary(cleanup.letters)).toBe("Freed T: (left over from SERVAULT\\King's closed vault).");
  expect(vaultLetterReportMessage(cleanup.letters[0])).toBe("T: was freed. It was left over from SERVAULT\\King's closed vault.");
  const { owner: _owner, ...unnamed } = king;
  expect(freedLettersSummary([{ ...freedT, holder: unnamed }])).toBe("Freed T: (left over from another Windows account's closed vault).");
  expect(freedLettersSummary([{ ...freedT, holder: { ...king, owner: undefined, owner_is_caller: true } }])).toBe("Freed T: (left over from your closed vault).");
  expect(freedLettersSummary([{ ...external, holder: ramDisk }])).toBeNull();
});

test("a letter that stays blocked says exactly what holds it and never asks anyone to sign out", () => {
  const messages = [
    vaultLetterReportMessage({ ...external, holder: { ...king, device_present: true } }),
    vaultLetterReportMessage({ ...external, letter: "R", holder: ramDisk }),
    vaultLetterReportMessage({ ...external, holder: { ...king, kind: "network_drive", logon_state: "active", device_present: true } }),
    vaultLetterReportMessage({ ...external, holder: { ...king, kind: "subst", logon_state: "signed_out", device_present: true } }),
    vaultLetterReportMessage({ ...external, holder: { kind: "windows_volume", scope: "machine", logon_state: "machine", device_present: true } }),
    vaultLetterReportMessage({ ...external, holder: { ...king, owner: undefined } }),
  ];
  expect(messages[0]).toBe("T: is held by a mounted encrypted volume for SERVAULT\\King (disconnected). Dismount it from that account, or choose another letter.");
  expect(messages[1]).toBe("R: is the WinCommander RAM disk. WinCommander left it unchanged; choose another letter.");
  expect(messages[2]).toContain("network drive mapped for SERVAULT\\King (signed in)");
  expect(messages[3]).toContain("subst for SERVAULT\\King (signed out). Remove it in that account (subst T: /d)");
  expect(messages[4]).toBe("T: is a Windows disk volume for all accounts on this PC. Choose another letter.");
  expect(messages[5]).toContain("another Windows account (disconnected)");
  for (const message of messages) expect(message.toLowerCase()).not.toContain("sign out");
});

test("holder details are accepted only in their verified shape", () => {
  for (const invalid of [
    { ...freedT, released: false },
    { ...freedT, category: "stale_wincommander_reservation" as const, holder: king, saved_entry: false, released: false },
    { ...external, holder: { ...king, owner_is_caller: true } },
    { ...external, holder: { ...king, kind: "bitlocker" } },
    { ...external, holder: { ...king, logon_state: "locked" } },
    { ...external, holder: { ...king, owner: "bad\u0007name" } },
    { ...external, category: "real_windows_volume" as const, holder: ramDisk },
  ]) {
    let error = "";
    try { readVaultLetterCleanup({ released: invalid.released ? 1 : 0, letters: [invalid] }); } catch (failure) { error = (failure as Error).message; }
    expect(error).toBe("vault_drive_letter_cleanup_unverified");
  }
});
