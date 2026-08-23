import type { VaultEntryResult, VaultPolicyStatus } from "./vaultAccessTypes";

export interface VaultPolicyVerification {
  tone: "success" | "warning";
  title: string;
  detail: string;
  appliedAt: number | null;
}

const DEGRADED_DETAIL: Record<VaultEntryResult, string> = {
  applied: "Windows couldn't verify the saved access settings. Mounting is unavailable until this is fixed. Check the container folders and Windows accounts, then save again.",
  pending_mount_broker: "The security service needs to refresh this older Vault status. Mounting is unavailable until this is fixed. Refresh, then save the settings again if the warning remains.",
  validation_failed: "Some saved Vault details are invalid. Mounting is unavailable until this is fixed. Check each required field, then save again.",
  principal_resolution_failed: "Windows couldn't find one or more named users or groups. Mounting is unavailable until this is fixed. Check the account names, then save again.",
  container_identity_failed: "Windows couldn't verify one or more encrypted container files. Mounting is unavailable until this is fixed. Check the container paths, then save again.",
  acl_apply_failed: "Windows couldn't apply the saved file permissions. Mounting is unavailable until this is fixed. Check the container folders and Windows accounts, then save again.",
  acl_readback_failed: "Windows couldn't confirm the saved file permissions. Mounting is unavailable until this is fixed. Check the container folders and Windows accounts, then save again.",
};

export function vaultPolicyVerification(status: VaultPolicyStatus | null): VaultPolicyVerification | null {
  if (!status || status.validation_state === "never_applied") return null;
  if (status.validation_state === "current") {
    return {
      tone: "success",
      title: "Saved to Windows",
      detail: "Windows verified the saved access settings. Mounting is checked separately when requested.",
      appliedAt: status.applied_at,
    };
  }

  const failure = status.entries.find(entry => entry.result !== "applied")?.result ?? "applied";
  return {
    tone: "warning",
    title: "Vault settings need attention",
    detail: DEGRADED_DETAIL[failure],
    appliedAt: null,
  };
}
