import { describe, expect, test } from "bun:test";
import { findVaultName, readVaultDisplayNames, vaultPathKey } from "./vaultRename";

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
