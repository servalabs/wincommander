import { describe, expect, test } from "bun:test";
import { vaultPolicySaveFailure } from "./VaultAccessTab";

describe("Vault policy save failures", () => {
  test("outsider administrator denial explains the group boundary before generic errors", () => {
    const failure = vaultPolicySaveFailure("vault_fleet_group_required: forbidden version conflict");
    expect(failure.code).toBe("VLT.POLICY.GROUP_REQUIRED");
    expect(failure.message).toContain("does not belong");
    expect(failure.message).toContain("administrator does not grant access");
    expect(failure.message).not.toContain("Refresh");
  });
  test("owner denial takes precedence over generic admin or stale-version wording", () => {
    const failure = vaultPolicySaveFailure("vault_owner_required: forbidden after version conflict");
    expect(failure.code).toBe("VLT.POLICY.OWNER_REQUIRED");
    expect(failure.message).toContain("Ask its owner");
    expect(failure.message).toContain("only while the Vault is unmounted");
    expect(failure.message).not.toContain("not a local administrator");
    expect(failure.message).not.toContain("draft");
  });
  test("shows a drive reservation conflict inline with a free-letter recovery action", () => {
    const failure = vaultPolicySaveFailure("Drive letter is occupied, reserved, or unavailable");
    expect(failure.code).toBe("VLT.POLICY.DRIVE_LETTER_CONFLICT");
    expect(failure.message).toContain("Refresh free letters");
  });
  test("does not call an unavailable drive check an occupied letter", () => {
    const failure = vaultPolicySaveFailure("vault_drive_letters_unavailable");
    expect(failure.code).toBe("VLT.POLICY.DRIVE_LETTER_UNAVAILABLE");
    expect(failure.message).toContain("could not check");
    expect(failure.message).not.toContain("occupied");
  });
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

  test("explains that local-administrator access is required without suggesting elevation", () => {
    const failure = vaultPolicySaveFailure(new Error("forbidden: vault policy operation requires Vault Policy Administrator"));

    expect(failure.code).toBe("VLT.POLICY.ADMIN_ACCESS_REQUIRED");
    expect(failure.message).toContain("not a local administrator");
    expect(failure.message).not.toContain("Run as administrator");
  });

  test("classifies Tauri's string rejection the same way as an Error", () => {
    const failure = vaultPolicySaveFailure("forbidden: vault policy operation requires Vault Policy Administrator");

    expect(failure.code).toBe("VLT.POLICY.ADMIN_ACCESS_REQUIRED");
    expect(failure.message).toContain("not a local administrator");
  });
});
