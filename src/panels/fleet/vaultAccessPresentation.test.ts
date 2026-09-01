import { describe, expect, test } from "bun:test";
import { vaultEntryResultLabel, vaultPolicyVerification } from "./vaultAccessPresentation";
import type { VaultEntryResult, VaultPolicyStatus } from "./vaultAccessTypes";

function status(
  validation_state: VaultPolicyStatus["validation_state"],
  result: VaultPolicyStatus["entries"][number]["result"] = "applied",
): VaultPolicyStatus {
  return {
    policy_id: "opaque-policy-id",
    version: 12,
    validation_state,
    applied_at: validation_state === "never_applied" ? null : 1_787_000_000,
    entries: validation_state === "never_applied" ? [] : [{ id: "vault-1", result }],
  };
}

describe("Vault policy verification presentation", () => {
  test("hides the untouched service default", () => {
    expect(vaultPolicyVerification(null)).toBeNull();
    expect(vaultPolicyVerification(status("never_applied"))).toBeNull();
  });

  test("shows a calm saved receipt without claiming a mount succeeded", () => {
    expect(vaultPolicyVerification(status("current"))).toEqual({
      tone: "success",
      title: "Saved to Windows",
      detail: "Windows verified the saved access settings. Mounting is checked separately when requested.",
      appliedAt: 1_787_000_000,
    });
  });

  test("turns degraded ACL status into an actionable fail-closed warning", () => {
    expect(vaultPolicyVerification(status("degraded", "acl_readback_failed"))).toEqual({
      tone: "warning",
      title: "Vault settings need attention",
      detail: "Windows couldn't confirm the saved file permissions. Mounting is unavailable until this is fixed. Check the container folders and Windows accounts, then save again.",
      appliedAt: null,
    });
  });

  test("keeps defensive future failures understandable without raw enum text", () => {
    const message = vaultPolicyVerification(status("degraded", "principal_resolution_failed"));
    expect(message?.detail).toContain("Windows couldn't find one or more named users or groups");
    expect(message?.detail).not.toContain("principal_resolution_failed");
    expect(JSON.stringify(message)).not.toContain("opaque-policy-id");
    expect(JSON.stringify(message)).not.toContain('"version":12');
  });
});

describe("vaultEntryResultLabel", () => {
  test("maps every per-entry result to plain administrator-facing text, never the raw enum", () => {
    const results: VaultEntryResult[] = [
      "applied", "pending_mount_broker", "validation_failed", "principal_resolution_failed",
      "container_identity_failed", "acl_apply_failed", "acl_readback_failed",
    ];
    for (const result of results) {
      const label = vaultEntryResultLabel(result);
      expect(label.length).toBeGreaterThan(0);
      expect(label).not.toBe(result);
      expect(label).not.toContain("_");
    }
  });

  test("names the specific failure instead of a generic message", () => {
    expect(vaultEntryResultLabel("principal_resolution_failed"))
      .toBe("A named Windows user or group could not be resolved");
    expect(vaultEntryResultLabel("applied")).toBe("Applied");
  });
});
