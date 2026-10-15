import { describe, expect, test } from "bun:test";
import { vaultPolicyReadFailure, vaultPolicyReviewConfirmed, vaultPolicySaveFailure } from "./VaultAccessTab";
import { newVaultPolicy, type VaultPolicyStatus } from "./vaultAccessTypes";

describe("Vault policy save failures", () => {
  test("review cannot clear an unknown save when status is missing, degraded, or a different revision", () => {
    const policy = { ...newVaultPolicy(), policy_id: "p", version: 4 };
    const status: VaultPolicyStatus = { policy_id: "p", version: 4, validation_state: "current", applied_at: null, entries: [] };
    expect(vaultPolicyReviewConfirmed(policy, status)).toBe(true);
    expect(vaultPolicyReviewConfirmed(policy, null)).toBe(false);
    expect(vaultPolicyReviewConfirmed(null, null)).toBe(false);
    expect(vaultPolicyReviewConfirmed(policy, { ...status, validation_state: "degraded" })).toBe(false);
    expect(vaultPolicyReviewConfirmed(null, { ...status, validation_state: "degraded" })).toBe(false);
    expect(vaultPolicyReviewConfirmed(policy, { ...status, version: 3 })).toBe(false);
    expect(vaultPolicyReviewConfirmed(policy, { ...status, policy_id: "other" })).toBe(false);
    expect(vaultPolicyReviewConfirmed(null, { ...status, validation_state: "never_applied" })).toBe(false);
    expect(vaultPolicyReviewConfirmed(null, { ...status, policy_id: null, version: 0, validation_state: "never_applied" })).toBe(true);
  });
  test("an ambiguous protected write remains unknown even with a service rejection receipt", () => {
    const failure = vaultPolicySaveFailure("service rejected request: vault_policy_commit_unconfirmed (raw secret detail)");
    expect(failure.code).toBe("VLT.POLICY.OUTCOME_UNKNOWN");
    expect(failure.message).toContain("Review the saved settings");
    expect(failure.message).not.toContain("secret");
    const rollback = vaultPolicySaveFailure("vault_policy_rollback_unverified raw secret detail");
    expect(rollback.code).toBe("VLT.POLICY.ROLLBACK_UNVERIFIED");
    expect(rollback.message).toContain("Access remains blocked");
    expect(rollback.message).not.toContain("unchanged");
  });
  test("unverified protected state preserves the draft instead of implying deletion", () => {
    const failure = vaultPolicySaveFailure("vault_policy_state_unavailable secret-path secret-account");
    expect(failure.code).toBe("VLT.POLICY.STATE_UNAVAILABLE");
    expect(failure.message).toContain("draft and saved version were kept");
    expect(failure.message).not.toContain("secret-");
    expect(failure.message).not.toContain("restart");
    expect(vaultPolicyReadFailure("vault_policy_state_unavailable secret-password")).toBe(failure.message);
    expect(vaultPolicyReadFailure("raw unexpected service error secret-path")).not.toContain("secret-");
  });
  test("persistence failure does not claim permission rollback or a successful save", () => {
    const failure = vaultPolicySaveFailure("service rejected request: vault_apply_failed (Vault policy could not be persisted)");
    expect(failure.code).toBe("VLT.POLICY.PERSISTENCE_FAILED");
    expect(failure.message).toContain("could not confirm");
    expect(failure.message).not.toContain("unchanged");
  });
  test("an accepted save with a lost reply remains unknown and never asks for a restart or replay", () => {
    for (const reason of ["vault_operation_unconfirmed", "service operation did not confirm before its deadline; refresh vault state before retrying", "service reply read failed: secret-value"] ) {
      const failure = vaultPolicySaveFailure(reason);
      expect(failure.code).toBe("VLT.POLICY.OUTCOME_UNKNOWN");
      expect(failure.message).toContain("may still be saving");
      expect(failure.message).not.toContain("no Vault settings were changed");
      expect(failure.message).not.toContain("secret-value");
      expect(failure.message.toLowerCase()).not.toContain("restart");
    }
  });
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
  test("tells a standard owner to use the administrator recreate workflow for transfer", () => {
    const failure = vaultPolicySaveFailure("vault_owner_transfer_requires_admin");
    expect(failure.code).toBe("VLT.POLICY.OWNER_TRANSFER_REQUIRES_ADMIN");
    expect(failure.message).toContain("cannot transfer ownership");
    expect(failure.message).toContain("administrator recreate");
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
