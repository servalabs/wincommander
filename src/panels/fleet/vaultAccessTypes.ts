/**
 * Frozen `svc.vault.*` JSON contract.
 *
 * This renderer type is a display/edit adapter only. The service resolves
 * accounts to SIDs, owns persistence, and reports bounded observations.
 */
export type VaultAccess = "read" | "write";
export type VaultPresentation = "machine" | "per-user";
export type VaultEntryResult =
  | "applied"
  | "pending_mount_broker"
  | "validation_failed"
  | "principal_resolution_failed"
  | "container_identity_failed"
  | "acl_apply_failed"
  | "acl_readback_failed";

export interface VaultGrantInput {
  principal_name: string;
  access: VaultAccess;
}

export interface VaultMountPolicy {
  presentation: VaultPresentation;
  preferred_letter?: string;
}

export interface VaultAccessEntry {
  id: string;
  label: string;
  container_path: string;
  container_identity?: string;
  owner_account: string;
  grants: VaultGrantInput[];
  mount: VaultMountPolicy;
}

export interface VaultAccessPolicy {
  schema_version: 1;
  policy_id: string;
  version: number;
  expected_previous_version: number;
  entries: VaultAccessEntry[];
}

export interface VaultEntryStatus {
  id: string;
  result: VaultEntryResult;
}

export interface VaultPolicyStatus {
  policy_id: string | null;
  version: number;
  validation_state: "never_applied" | "current" | "degraded";
  applied_at: number | null;
  entries: VaultEntryStatus[];
}

export function newVaultEntry(kind: "shared" | "private" = "private"): VaultAccessEntry {
  const id = typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : `vault-${Date.now().toString(36)}`;
  const shared = kind === "shared";
  return {
    id,
    label: shared ? "Shared vault" : "Administrator vault",
    container_path: "",
    owner_account: "Administrator",
    grants: shared
      ? [{ principal_name: "Administrator", access: "write" }, { principal_name: "Partner", access: "write" }]
      : [{ principal_name: "Administrator", access: "write" }],
    mount: { presentation: shared ? "machine" : "per-user" },
  };
}

export function newVaultPolicy(): VaultAccessPolicy {
  const policyId = typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : `vault-policy-${Date.now().toString(36)}`;
  return {
    schema_version: 1,
    policy_id: policyId,
    version: 0,
    expected_previous_version: 0,
    entries: [newVaultEntry("shared"), newVaultEntry("private"), newVaultEntry("private")],
  };
}

export function validateVaultAccessIntent(policy: VaultAccessPolicy): string | null {
  if (!policy.policy_id.trim()) return "A policy identifier is required.";
  if (policy.entries.length === 0) return "Add at least one file-container vault.";
  if (policy.entries.some(entry => !entry.label.trim() || !entry.container_path.trim() || !entry.owner_account.trim())) {
    return "Every vault needs a label, container path, and owner account.";
  }
  if (policy.entries.some(entry => entry.grants.length === 0 || entry.grants.some(grant => !grant.principal_name.trim()))) {
    return "Every vault needs at least one named grant.";
  }
  if (policy.entries.some(entry => entry.mount.preferred_letter && !/^[A-Z]$/i.test(entry.mount.preferred_letter))) {
    return "Preferred drive letters must be one letter from A to Z.";
  }
  const parents = policy.entries.map(entry => containerParent(entry.container_path));
  if (parents.some(parent => !parent)) {
    return "Every file container must be inside its own dedicated parent folder.";
  }
  if (new Set(parents.map(parent => parent!.toLocaleLowerCase())).size !== parents.length) {
    return "Each managed container needs its own dedicated parent folder; vaults cannot share a parent.";
  }
  return null;
}

function containerParent(containerPath: string): string | null {
  const normalized = containerPath.trim().replaceAll("/", "\\").replace(/\\+$/, "");
  const separator = normalized.lastIndexOf("\\");
  // A drive root is never a dedicated parent folder for managed containers.
  if (separator <= 2) return null;
  return normalized.slice(0, separator) || null;
}
