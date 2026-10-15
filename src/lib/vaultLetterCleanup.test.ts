// SPDX-License-Identifier: AGPL-3.0-or-later
import { expect, test } from "bun:test";
import { readVaultLetterCleanup, vaultLetterReportMessage } from "./vaultLetterCleanup";
const stale = { letter: "Q", category: "stale_wincommander_reservation" as const, released: true, saved_entry: false };
test("cleanup success requires a matching bounded ownership classification", () => {
  expect(readVaultLetterCleanup({ released: 1, letters: [stale] }).released).toBe(1);
  for (const result of [{ released: 1 }, { released: 0, letters: [stale] }, { released: 1, letters: [{ ...stale, letter: "C" }] },
    { released: 1, letters: [{ ...stale, saved_entry: true }] }, { released: 1, letters: [{ ...stale, category: "external_or_unknown_owner" }] }]) {
    let error = ""; try { readVaultLetterCleanup(result); } catch (failure) { error = (failure as Error).message; }
    expect(error).toBe("vault_drive_letter_cleanup_unverified");
  }
});
test("unreleasable letters explain their safe next action without raw service data", () => {
  expect(vaultLetterReportMessage({ ...stale, letter: "F", released: false, category: "real_windows_volume" })).toContain("mounted Windows volume");
  expect(vaultLetterReportMessage({ ...stale, released: false, category: "external_or_unknown_owner" })).toContain("left it unchanged");
  expect(vaultLetterReportMessage({ ...stale, released: false, category: "duplicate_saved_entry" })).toContain("duplicate");
});
