import { describe, expect, test } from "bun:test";
import {
  newVaultEntry,
  newVaultPolicy,
  validateVaultAccessIntent,
  vaultMountResultLabel,
  vaultPresentationLabel,
} from "./vaultAccessTypes";

describe("Vault Access service intent", () => {
  test("starts with the shared plus two private file-container shape", () => {
    const policy = newVaultPolicy();
    expect(policy.schema_version).toBe(1);
    expect(policy.entries).toHaveLength(3);
    expect(policy.entries[0]?.mount.presentation).toBe("machine");
    expect(policy.entries.slice(1).every(entry => entry.mount.presentation === "per-user")).toBe(true);
    expect(policy.entries.every(entry => entry.container_kind === "standard")).toBe(true);
  });

  test("does not treat an incomplete renderer draft as deployable", () => {
    const policy = newVaultPolicy();
    expect(validateVaultAccessIntent(policy)).toBe("Every vault needs a label, container path, and owner account.");
  });

  test("uses owner-only defaults and permits an explicit empty revoke policy", () => {
    expect(newVaultEntry("shared").grants).toEqual([{ principal_name: "Administrator", access: "write" }]);
    const policy = newVaultPolicy();
    policy.entries = [];
    expect(validateVaultAccessIntent(policy)).toBeNull();
  });

  test("rejects a Shared vault left with only the owner's default grant", () => {
    // newVaultEntry("shared") is the app's own starter shape: machine
    // presentation with exactly one grant. The service rejects that
    // combination outright, so the draft must be caught before Apply.
    const policy = newVaultPolicy();
    const entry = policy.entries[0]!;
    policy.entries = [entry];
    entry.container_path = "C:\\Vaults\\shared\\vault.hc";
    expect(entry.mount.presentation).toBe("machine");
    expect(entry.grants).toHaveLength(1);
    expect(validateVaultAccessIntent(policy)).toBe(
      "A Shared vault needs at least two named grants — add a second person or group, or switch it to Personal vault.",
    );
  });

  test("rejects whitespace and case insensitive duplicate grants", () => {
    const policy = newVaultPolicy();
    const entry = policy.entries[0]!;
    policy.entries = [entry];
    entry.container_path = "C:\\Vaults\\shared\\vault.hc";
    entry.grants = [
      { principal_name: "DOMAIN\\Partner", access: "read" },
      { principal_name: " domain\\partner ", access: "write" },
    ];
    expect(validateVaultAccessIntent(policy)).toBe(
      "A vault cannot grant the same Windows user or group more than once.",
    );
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
    // entries[0] is the machine-presentation "shared" starter; give it a
    // second grant so this test isolates the parent-folder collision it
    // targets, rather than tripping the separate too-few-grants check.
    policy.entries[0]!.grants.push({ principal_name: "Partner", access: "read" });
    policy.entries.forEach((entry, index) => { entry.container_path = `C:\\Vaults\\vault-${index}.hc`; });
    expect(validateVaultAccessIntent(policy)).toBe(
      "Each managed container needs its own dedicated parent folder; vaults cannot share a parent.",
    );
  });

  test("allows a dual outer plus hidden container to be shared through Fleet policy", () => {
    const policy = newVaultPolicy();
    const entry = policy.entries[1]!;
    policy.entries = [entry];
    entry.label = "Kaushal private";
    entry.container_path = "K:\\Vaults\\Kaushal\\private.hc";
    entry.owner_account = "Kaushal";
    entry.container_kind = "dual";
    entry.grants = [
      { principal_name: "Kaushal", access: "write" },
      { principal_name: "Partner", access: "read" },
    ];
    entry.mount.presentation = "machine";
    expect(validateVaultAccessIntent(policy)).toBeNull();
  });

  test("renders bounded mount lifecycle information without container details", () => {
    expect(vaultPresentationLabel("per-user")).toBe("Private or decoy Vault — only this signed-in user");
    expect(vaultPresentationLabel("machine")).toBe("Shared Vault — available to authorized users");
    expect(vaultMountResultLabel({
      entry_id: "opaque-entry",
      state: "mounted",
      presentation: "per-user",
      drive_letter: "V:",
      reason: null,
    })).toBe("Mounted at V:");
    expect(vaultMountResultLabel({
      entry_id: "opaque-entry",
      state: "denied",
      presentation: null,
      drive_letter: null,
      reason: "not_authorized",
    })).toBe("Mount denied by the secure service");
  });
});
