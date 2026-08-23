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

  test("keeps the named principals while setting one shared access level", () => {
    const entry = newVaultEntry("shared");
    const readOnly = applyVaultAccessPreset(entry, "shared-read");
    const editable = applyVaultAccessPreset(readOnly, "shared-write");

    expect(readOnly.mount.presentation).toBe("machine");
    expect(readOnly.grants.every(grant => grant.access === "read")).toBe(true);
    expect(editable.grants.map(grant => grant.principal_name)).toEqual(readOnly.grants.map(grant => grant.principal_name));
    expect(vaultAccessPreset(editable)).toBe("shared-write");
  });

  test("labels mixed grants as custom without changing their policy intent", () => {
    const entry = newVaultEntry("shared");
    entry.grants.push({ principal_name: "PC\\Readers", access: "read" });

    expect(vaultAccessPreset(entry)).toBe("custom");
  });
});
