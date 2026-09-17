import { describe, expect, test } from "bun:test";
import { vaultPolicySaveFailure } from "./VaultAccessTab";

describe("Vault policy save failures", () => {
  test("keeps a malformed service policy reply actionable without exposing its raw details", () => {
    const failure = vaultPolicySaveFailure(new Error("vault_apply_failed: vault policy failed validation — check drive letters"));

    expect(failure.code).toBe("VLT.POLICY.INVALID");
    expect(failure.message).toContain("rejected these Vault settings");
    expect(failure.message).not.toContain("vault_apply_failed");
  });

  test("separates missing Windows principals from a generic save failure", () => {
    const failure = vaultPolicySaveFailure(new Error("vault_apply_failed: vault principal resolution failed for 'Old account'"));

    expect(failure.code).toBe("VLT.POLICY.PRINCIPAL_UNAVAILABLE");
    expect(failure.message).toContain("Access control");
    expect(failure.message).not.toContain("Old account");
  });

  test("does not tell a person to retry a policy change while a mounted vault is still in use", () => {
    const failure = vaultPolicySaveFailure(new Error("vault_dismount_failed: active vaults could not be dismounted"));

    expect(failure.code).toBe("VLT.POLICY.ACTIVE_MOUNT");
    expect(failure.message).toContain("Close files");
  });

  test("states when the developer service has not accepted a save", () => {
    const failure = vaultPolicySaveFailure(new Error("service pipe timed out"));

    expect(failure.code).toBe("VLT.POLICY.SERVICE_UNAVAILABLE");
    expect(failure.message).toContain("no Vault settings were changed");
  });

  test("explains missing Vault policy-manager membership without suggesting elevation", () => {
    const failure = vaultPolicySaveFailure(new Error("forbidden: vault policy operation requires Vault Policy Administrator"));

    expect(failure.code).toBe("VLT.POLICY.ADMIN_ACCESS_REQUIRED");
    expect(failure.message).toContain("WinCommander Vault Policy Administrators");
    expect(failure.message).not.toContain("Run as administrator");
  });

  test("classifies Tauri's string rejection the same way as an Error", () => {
    const failure = vaultPolicySaveFailure("forbidden: vault policy operation requires Vault Policy Administrator");

    expect(failure.code).toBe("VLT.POLICY.ADMIN_ACCESS_REQUIRED");
    expect(failure.message).toContain("not allowed to change Vault settings");
  });
});
