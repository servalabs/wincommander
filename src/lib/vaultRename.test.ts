import { describe, expect, test } from "bun:test";
import {
  MAX_VAULT_NAME_BYTES,
  findVaultName,
  readVaultDisplayNames,
  vaultNameProblem,
  vaultPathKey,
  vaultRenameErrorMessage,
} from "./vaultRename";

describe("vaultNameProblem", () => {
  test("accepts a trimmed name up to the service byte limit", () => {
    expect(vaultNameProblem("  Taxes 2026  ")).toBeNull();
    expect(vaultNameProblem("Ünïcode ✓")).toBeNull();
    expect(vaultNameProblem("a".repeat(MAX_VAULT_NAME_BYTES))).toBeNull();
  });

  test("rejects an empty, blank or too long name", () => {
    expect(vaultNameProblem("")).toBe("Enter a name.");
    expect(vaultNameProblem("   ")).toBe("Enter a name.");
    expect(vaultNameProblem("a".repeat(MAX_VAULT_NAME_BYTES + 1))).toContain("too long");
    // 43 three-byte characters are 129 bytes although only 43 characters.
    expect(vaultNameProblem("✓".repeat(43))).toContain("too long");
  });

  test("rejects control characters inside the name", () => {
    for (const name of ["two\nlines", "tab\there", "nul\u0000", "bell\u0007"]) {
      expect(vaultNameProblem(name)).toContain("control characters");
    }
  });
});

describe("vault name lookup", () => {
  const names = [{ path: "D:\\Vaults\\Notes.hc", name: "Taxes" }];

  test("matches the same file across case, slashes and the verbatim prefix", () => {
    expect(vaultPathKey("\\\\?\\D:/Vaults/NOTES.hc")).toBe("d:\\vaults\\notes.hc");
    expect(vaultPathKey("\\??\\D:\\Vaults\\notes.hc")).toBe("d:\\vaults\\notes.hc");
    expect(findVaultName(names, "d:/vaults/notes.HC")).toBe("Taxes");
  });

  test("returns null for another file or no path", () => {
    expect(findVaultName(names, "D:\\Vaults\\other.hc")).toBeNull();
    expect(findVaultName(names, null)).toBeNull();
    expect(findVaultName(names, " ")).toBeNull();
  });

  test("drops malformed rows and rejects a reply without a list", () => {
    expect(readVaultDisplayNames({ names: [{ path: "D:\\a.hc", name: "A" }, { path: "D:\\b.hc" }, null, { path: "", name: "C" }] }))
      .toEqual([{ path: "D:\\a.hc", name: "A" }]);
    for (const reply of [{}, null, { names: "x" }]) {
      let failed = false;
      try { readVaultDisplayNames(reply); } catch { failed = true; }
      expect(failed).toBe(true);
    }
  });
});

describe("vaultRenameErrorMessage", () => {
  test("explains each service refusal in plain words", () => {
    const cases: Array<[string, string]> = [
      ["service rejected request: vault_not_registered (WinCommander has no record)", "no record of this vault"],
      ["service rejected request: vault_not_authorized (caller cannot rename this Vault)", "cannot rename this vault"],
      ["service rejected request: volume_mounted (dismount the Vault before renaming it)", "Dismount the vault first"],
      ["vault_mount_state_unknown", "could not confirm"],
      ["vault_rename_not_saved", "keeps its old name"],
      ["service operation did not confirm before its deadline", "did not get a result in time"],
    ];
    for (const [error, expected] of cases) expect(vaultRenameErrorMessage(new Error(error))).toContain(expected);
  });

  test("never echoes unknown error text", () => {
    expect(vaultRenameErrorMessage(new Error("C:\\secret\\path failed"))).toBe("The vault was not renamed. Check the vault file and try again.");
  });
});
