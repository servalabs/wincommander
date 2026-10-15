// SPDX-License-Identifier: AGPL-3.0-or-later
import { expect, test } from "bun:test";
import { changeQuickMountSlot } from "./quickMountSlots";

const errorOf = (work: () => unknown) => { try { work(); return ""; } catch (error) { return (error as Error).message; } };
const a = { filePath: "fixture-a", driveLetter: "Q" };
const b = { filePath: "fixture-b", driveLetter: "R" };
test("deleting a shortcut preserves a newer saved entry and releases only its preference", () => {
  expect(changeQuickMountSlot([a, b], a, null)).toEqual([b]);
});
test("a queued save uses the latest reservations and rejects a duplicate letter", () => {
  expect(errorOf(() => changeQuickMountSlot([a], null, { ...b, driveLetter: "q:" }))).toContain("another saved");
  expect(changeQuickMountSlot([a], null, b)).toEqual([a, b]);
});
test("missing, replaced and duplicate shortcut identities cannot select another record", () => {
  for (const records of [[b], [{ ...a, driveLetter: "S" }]]) {
    expect(errorOf(() => changeQuickMountSlot(records, a, null))).toContain("Refresh Quick Mount");
  }
  expect(changeQuickMountSlot([a, a], a, null)).toEqual([a]);
  expect(errorOf(() => changeQuickMountSlot([a, a], a, b))).toContain("duplicate records");
});
test("saving never reserves a protected or malformed letter", () => {
  for (const driveLetter of ["A", "B", "C", "QQ", "", "?"]) {
    expect(errorOf(() => changeQuickMountSlot([], null, { ...a, driveLetter }))).toContain("non-system");
  }
});
