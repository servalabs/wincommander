import { describe, expect, test } from "bun:test";
import { newVaultPolicy, validateVaultAccessIntent } from "./vaultAccessTypes";

describe("Vault Access service intent", () => {
  test("starts with the shared plus two private file-container shape", () => {
    const policy = newVaultPolicy();
    expect(policy.schema_version).toBe(1);
    expect(policy.entries).toHaveLength(3);
    expect(policy.entries[0]?.mount.presentation).toBe("machine");
    expect(policy.entries.slice(1).every(entry => entry.mount.presentation === "per-user")).toBe(true);
  });

  test("does not treat an incomplete renderer draft as deployable", () => {
    const policy = newVaultPolicy();
    expect(validateVaultAccessIntent(policy)).toBe("Every vault needs a label, container path, and owner account.");
  });

  test("submits a next version while guarding against the version it edited", () => {
    const observed = newVaultPolicy();
    const submitted = {
      ...observed,
      expected_previous_version: observed.version,
      version: observed.version + 1,
    };
    expect(submitted.expected_previous_version).toBe(0);
    expect(submitted.version).toBe(1);
  });

  test("rejects two managed containers in one parent folder", () => {
    const policy = newVaultPolicy();
    policy.entries.forEach((entry, index) => { entry.container_path = `C:\\Vaults\\vault-${index}.hc`; });
    expect(validateVaultAccessIntent(policy)).toBe(
      "Each managed container needs its own dedicated parent folder; vaults cannot share a parent.",
    );
  });
});
