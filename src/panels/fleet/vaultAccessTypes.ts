/**
 * Frozen `svc.vault.*` JSON contract.
 *
 * This renderer type is a display/edit adapter only. The service resolves
 * accounts to SIDs, owns persistence, and reports bounded observations.
 */
export type VaultAccess = "read" | "write";
export type VaultPresentation = "machine" | "per-user";
/** `dual` is a VeraCrypt outer + hidden pair in one container file. */
export type VaultContainerKind = "standard" | "dual";
/** Chosen for one mount request; this is never persisted with the policy. */
export type VaultVolumeRole = "outer" | "hidden";
export type VaultEntryResult =
  | "applied"
  | "pending_mount_broker"
  | "validation_failed"
  | "principal_resolution_failed"
  | "container_identity_failed"
  | "acl_apply_failed"
  | "acl_readback_failed";

/** Bounded renderer-facing lifecycle reported by the secure mount broker. */
export type VaultMountState = "mounted" | "unmounted" | "denied" | "failed";
export const VAULT_MOUNT_REASONS = [
  "not_authorized",
  "invalid_request",
  "broker_unavailable",
  "broker_rejected",
  "session_unavailable",
  "engine_unlock_failed",
  "engine_drive_letter_unavailable",
  "engine_mount_failed",
  "acl_apply_failed",
  "acl_readback_failed",
  "dismount_failed",
] as const;
export type VaultMountReason = typeof VAULT_MOUNT_REASONS[number];

export interface VaultGrantInput {
  principal_name: string;
  access: VaultAccess;
}

export interface VaultMountPolicy {
  presentation: VaultPresentation;
  preferred_letter?: string | null;
}

export interface VaultAccessEntry {
  id: string;
  label: string;
  container_path: string;
  container_identity?: string | null;
  container_kind: VaultContainerKind;
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
  mount_state?: VaultMountState;
}

export interface VaultPolicyStatus {
  policy_id: string | null;
  version: number;
  validation_state: "never_applied" | "current" | "degraded";
  applied_at: number | null;
  entries: VaultEntryStatus[];
}

/**
 * The desktop bridge intentionally returns no container location, identity,
 * SID, or ACL information. The service remains the authorization boundary.
 */
export interface VaultMountEntryResult {
  entry_id: string;
  state: VaultMountState;
  presentation: VaultPresentation | null;
  drive_letter: string | null;
  reason: VaultMountReason | null;
}

/** Caller-filtered mount view. It intentionally omits policy and filesystem data. */
export interface VaultAuthorizedEntry {
  entry_id: string;
  label: string;
  access: VaultAccess;
  presentation: VaultPresentation;
  container_kind: VaultContainerKind;
  mount_state: VaultMountState;
  drive_letter: string | null;
}

/** Service-derived caller capability; never infer this from cached machine state. */
export interface VaultAccessCapabilities {
  can_manage_policy: boolean;
}

export function vaultPresentationLabel(presentation: VaultPresentation | null | undefined): string {
  return presentation === "machine"
    ? "Shared Vault — available to authorized users"
    : "Private or decoy Vault — only this signed-in user";
}

export function vaultMountResultLabel(result: VaultMountEntryResult): string {
  if (result.state === "mounted") {
    return result.drive_letter
      ? `Mounted at ${result.drive_letter}`
      : "Mounted for this Windows session";
  }
  if (result.state === "unmounted") return "Unmounted";
  const labels: Record<VaultMountReason, string> = {
    not_authorized: "Mount denied by the secure service",
    invalid_request: "The Vault request is invalid",
    broker_unavailable: "The secure Vault service is unavailable",
    broker_rejected: "The secure Vault service rejected the request",
    session_unavailable: "The Windows session is unavailable",
    engine_unlock_failed: "The password, PIM, or keyfiles did not unlock this Vault",
    engine_drive_letter_unavailable: "The requested drive letter is already in use",
    engine_mount_failed: "The encrypted-volume engine could not mount this Vault",
    acl_apply_failed: "Windows permissions could not be applied to this Vault",
    acl_readback_failed: "Windows permissions could not be verified for this Vault",
    dismount_failed: "The Vault could not be safely unmounted",
  };
  return result.reason ? labels[result.reason] : "Mount request could not be completed";
}

export function newVaultEntry(kind: "shared" | "private" = "private"): VaultAccessEntry {
  const id = typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : fallbackId("vault");
  const shared = kind === "shared";
  return {
    id,
    label: shared ? "Shared vault" : "Administrator vault",
    container_path: "",
    container_kind: "standard",
    owner_account: "Administrator",
    // A shared presentation changes only where Windows exposes the mounted
    // drive. It must not silently grant a generic local account write access.
    grants: [{ principal_name: "Administrator", access: "write" }],
    mount: { presentation: shared ? "machine" : "per-user" },
  };
}

export function newVaultPolicy(): VaultAccessPolicy {
  const policyId = typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : fallbackId("vault-policy");
  return {
    schema_version: 1,
    policy_id: policyId,
    version: 0,
    expected_previous_version: 0,
    entries: [newVaultEntry("shared"), newVaultEntry("private"), newVaultEntry("private")],
  };
}

/** Older saved policies predate container kinds; their safe interpretation is standard. */
export function normalizeVaultAccessPolicy(policy: VaultAccessPolicy): VaultAccessPolicy {
  return {
    ...policy,
    entries: policy.entries.map(entry => {
      const legacy = entry as VaultAccessEntry & { volume_kind?: VaultContainerKind };
      const { volume_kind: _legacyVolumeKind, ...normalized } = legacy;
      return {
        ...normalized,
        container_kind: legacy.container_kind ?? legacy.volume_kind ?? "standard",
      };
    }),
  };
}

export function validateVaultAccessIntent(policy: VaultAccessPolicy): string | null {
  if (!policy.policy_id.trim()) return "A policy identifier is required.";
  // An empty policy is the explicit emergency/decommission state: the service
  // atomically removes the prior policy and its active mounts before accepting it.
  if (policy.entries.length === 0) return null;
  if (policy.entries.some(entry => !entry.label.trim() || !entry.container_path.trim() || !entry.owner_account.trim())) {
    return "Every vault needs a label, container path, and owner account.";
  }
  if (policy.entries.some(entry => entry.container_kind !== "standard" && entry.container_kind !== "dual")) {
    return "Every vault must use a standard or dual container type.";
  }
  if (policy.entries.some(entry => entry.grants.length === 0 || entry.grants.some(grant => !grant.principal_name.trim()))) {
    return "Every vault needs at least one named grant.";
  }
  // The service requires two or more grants for a machine-presented ("Shared")
  // vault — a single grant there is indistinguishable from a private vault and
  // is rejected server-side. Catch it here so Apply doesn't round-trip for it.
  if (policy.entries.some(entry => entry.mount.presentation === "machine" && entry.grants.length < 2)) {
    return "A Shared vault needs at least two named grants — add a second person or group, or switch it to Personal vault.";
  }
  if (policy.entries.some(entry => {
    const principals = entry.grants.map(grant => grant.principal_name.trim().toLocaleLowerCase());
    return new Set(principals).size !== principals.length;
  })) {
    return "A vault cannot grant the same Windows user or group more than once.";
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

let fallbackSequence = 0;

function fallbackId(prefix: string): string {
  fallbackSequence = (fallbackSequence + 1) % Number.MAX_SAFE_INTEGER;
  return `${prefix}-${Date.now().toString(36)}-${fallbackSequence.toString(36)}`;
}

function containerParent(containerPath: string): string | null {
  const normalized = containerPath.trim().replaceAll("/", "\\").replace(/\\+$/, "");
  const separator = normalized.lastIndexOf("\\");
  // A drive root is never a dedicated parent folder for managed containers.
  if (separator <= 2) return null;
  return normalized.slice(0, separator) || null;
}
