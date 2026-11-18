// SPDX-License-Identifier: AGPL-3.0-or-later
import { expect, test } from "bun:test";
import { addPickedVaultSources, pickedVaultPath, restorePathInPickedFolder } from "./vaultCloudBackupPicker";

test("picker accepts only safe paths inside the mounted Vault", () => {
  expect(pickedVaultPath("X:", "x:\\Documents\\Report.txt")).toBe("Documents\\Report.txt");
  expect(pickedVaultPath("X:", "X:/Documents/Photos/")).toBe("Documents\\Photos");
  for (const path of ["Y:\\Documents", "X:\\..\\secret", "X:\\.wincommander\\session", "X:\\$RECYCLE.BIN", "X:\\", "\\\\server\\share", "X:\\Documents\\file:stream"]) {
    expect(pickedVaultPath("X:", path)).toBeNull();
  }
  expect(pickedVaultPath("X:", "X:\\", true)).toBe("");
  expect(pickedVaultPath("not-a-drive", "X:\\Docs")).toBeNull();
});

test("picker appends and deduplicates sources without accepting mixed drives", () => {
  expect(addPickedVaultSources("X:", "Docs", ["x:\\docs", "X:\\photo.txt"])).toBe("Docs\nphoto.txt");
  expect(addPickedVaultSources("X:", "Docs", ["X:\\photo.txt", "Y:\\secret"])).toBeNull();
});

test("restore picker creates a new folder below the chosen parent", () => {
  expect(restorePathInPickedFolder("X:", "X:\\Reports", "")).toBe("Reports\\Restored files");
  expect(restorePathInPickedFolder("X:", "X:\\Reports", "Old\\My recovery")).toBe("Reports\\My recovery");
  expect(restorePathInPickedFolder("X:", "X:\\", "")).toBe("Restored files");
  expect(restorePathInPickedFolder("X:", "Y:\\Reports", "")).toBeNull();
});
