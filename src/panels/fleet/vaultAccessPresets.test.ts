import { describe, expect, test } from "bun:test";
import { newVaultEntry } from "./vaultAccessTypes";
import { applyVaultAccessPreset, vaultAccessPreset } from "./vaultAccessPresets";

describe("Vault access presets", () => {
  test("makes a personal vault a per-user, owner-only write mount", () => {
    const entry = newVaultEntry("shared");
    entry.owner_account = "PC\\Owner";
    const personal = applyVaultAccessPreset(entry, "private");

    expect(personal.mount.presentation).toBe("per-user");
    expect(personal.grants).toEqual([{ principal_name: "PC\\Owner", access: "write" }]);
    expect(vaultAccessPreset(personal)).toBe("private");
  });

  test("keeps the owner writable while making every other shared-read grant read-only", () => {
    const entry = newVaultEntry("shared");
    entry.owner_account = "PC\\Owner";
    entry.grants = [
      { principal_name: "PC\\Owner", access: "write" },
      { principal_name: "PC\\Readers", access: "write" },
    ];
    const readOnly = applyVaultAccessPreset(entry, "shared-read");
    const editable = applyVaultAccessPreset(readOnly, "shared-write");

    expect(readOnly.mount.presentation).toBe("machine");
    expect(readOnly.grants).toEqual([
      { principal_name: "PC\\Owner", access: "write" },
      { principal_name: "PC\\Readers", access: "read" },
    ]);
    expect(vaultAccessPreset(readOnly)).toBe("shared-read");
    expect(editable.grants.map(grant => grant.principal_name)).toEqual(readOnly.grants.map(grant => grant.principal_name));
    expect(vaultAccessPreset(editable)).toBe("shared-write");
  });

  test("adds an explicit owner write grant when turning an existing share view-only", () => {
    const entry = newVaultEntry("shared");
    entry.owner_account = "PC\\Owner";
    entry.grants = [{ principal_name: "PC\\Readers", access: "write" }];

    expect(applyVaultAccessPreset(entry, "shared-read").grants).toEqual([
      { principal_name: "PC\\Owner", access: "write" },
      { principal_name: "PC\\Readers", access: "read" },
    ]);
  });

  test("labels extra non-owner writers as custom without changing their policy intent", () => {
    const entry = newVaultEntry("shared");
    entry.grants.push({ principal_name: "PC\\Editors", access: "write" });
    entry.grants.push({ principal_name: "PC\\Readers", access: "read" });

    expect(vaultAccessPreset(entry)).toBe("custom");
  });
});
