/**
 * Frozen `svc.vault.*` JSON contract.
 *
 * This renderer type is a display/edit adapter only. The service resolves
 * accounts to SIDs, owns persistence, and reports bounded observations.
 */
export type VaultAccess = "read" | "write";
export type VaultPresentation = "machine" | "per-user";
/** Durable UI intent for the three access-selector choices. The service still
 * derives and enforces the real grants and mount presentation. */
export type VaultAccessPattern = "private" | "shared-read" | "shared-write";
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
  "administrator_required",
  "policy_access_denied",
  "private_owner_required",
  "mount_state_unknown",
  "invalid_request",
  "already_mounted",
  "broker_unavailable",
  "broker_rejected",
  "broker_identity_rejected",
  "broker_handshake_rejected",
  "broker_reply_rejected",
  "broker_plan_rejected",
  "presentation_rejected",
  "caller_access_denied",
  "caller_acl_repair_failed",
  "entitlement_denied",
  "pro_not_installed",
  "session_unavailable",
  "syncthing_profile_unavailable",
  "syncthing_install_failed",
  "syncthing_root_conflict",
  "syncthing_not_enabled",
  "engine_unlock_failed",
  "engine_drive_letter_unavailable",
  "engine_mount_failed",
  "acl_apply_failed",
  "acl_readback_failed",
  "dismount_failed",
] as const;
export type VaultMountReason = typeof VAULT_MOUNT_REASONS[number];

const VAULT_MOUNT_REASON_LABELS: Record<VaultMountReason, string> = {
  not_authorized: "This Windows account cannot use this Vault. Ask its owner to review your access. An administrator may remove its policy only while it is unmounted; that does not grant access to its contents",
  administrator_required: "This machine-wide drive needs administrator approval. Open WinCommander as an administrator to manage it",
  policy_access_denied: "Your Windows account does not have the required Fleet Vault permission. Ask the Vault owner to review your access",
  private_owner_required: "This private Vault is available only to its owner in the Windows session where it was mounted; administrator access does not replace ownership",
  mount_state_unknown: "This drive's mount identity could not be verified for your Windows account. Refresh Secure Storage. If this persists after an upgrade or an external mount, close open files and have the authorized owner dismount it using the original mounting tool, then restart the WinCommander service before remounting",
  invalid_request: "The Vault request is invalid",
  already_mounted: "This Vault is already mounted. Open the existing drive, or dismount it before changing its mount settings",
  broker_unavailable: "The secure Vault service is unavailable",
  broker_rejected: "The secure Vault service rejected the request",
  broker_identity_rejected: "The secure Vault helper identity could not be verified",
  broker_handshake_rejected: "The secure Vault helper handshake could not be verified",
  broker_reply_rejected: "The secure Vault helper reply could not be verified",
  broker_plan_rejected: "The secure Vault helper rejected the service-generated mount plan",
  presentation_rejected: "The Vault drive was not available to this signed-in Windows account",
  caller_access_denied: "Windows did not allow this account to read the mounted Vault contents",
  caller_acl_repair_failed: "The current Windows account could not be added to this Vault's existing permissions",
  entitlement_denied: "A valid Pro licence is required for this Vault operation. Open License settings and verify or activate your key",
  pro_not_installed: "The Pro module is not installed. Open License / Pro and install it before mounting. Activating a licence alone does not install the encryption engine",
  session_unavailable: "The Windows session is unavailable",
  syncthing_profile_unavailable: "Syncthing did not become ready for this Windows account. Keep the personal Vault mounted, then retry. If it persists, verify the Syncthing installation",
  syncthing_install_failed: "Syncthing could not be installed for this Windows account. Check your internet connection and Windows installation restrictions, then retry",
  syncthing_root_conflict: "The saved Syncthing folder location conflicts with this Vault or another sync folder. Review the existing folder settings before retrying",
  syncthing_not_enabled: "Sync is not enabled for this personal Vault. In Vault access, edit this Vault, turn on Syncthing, save the Vault policy, then try again",
  engine_unlock_failed: "The password, PIM, or keyfiles did not unlock this Vault",
  engine_drive_letter_unavailable: "The requested drive letter is already in use",
  engine_mount_failed: "The encrypted-volume engine could not mount this Vault",
  acl_apply_failed: "Windows permissions could not be applied to this Vault",
  acl_readback_failed: "Windows permissions could not be verified for this Vault",
  dismount_failed: "The Vault could not be safely unmounted",
};

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
  /** Stable owner identity selected from the service directory. Never derive
   * authorization from the display name in the renderer. */
  primary_owner_sid?: string | null;
  /** Service-confirmed path state. A renderer draft has neither field. */
  container_path_state?: "available" | "unavailable";
  canonical_container_path?: string | null;
  container_identity?: string | null;
  container_kind: VaultContainerKind;
  owner_account: string;
  /** Preserves the exact selector choice when a one-owner shared policy has
   * grants that would otherwise be indistinguishable. */
  access_pattern?: VaultAccessPattern | null;
  /** Explicit consent for Syncthing to manage this personal Vault. Missing is
   * a pre-control policy and is discovered safely once by the service. */
  syncthing_opt_in?: boolean | null;
  /** Service-derived UI capability only. It is stripped from every write. */
  can_edit_policy?: boolean;
  /** An outsider administrator may remove an unmounted policy without gaining edit access. */
  can_remove_policy?: boolean;
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
  sync_warning?: import("@/lib/vaultSyncWarning").VaultSyncWarning | null;
}

/** Caller-filtered mount view. It intentionally omits policy and filesystem data. */
export interface VaultAuthorizedEntry {
  entry_id: string;
  label: string;
  access: VaultAccess;
  presentation: VaultPresentation;
  container_kind: VaultContainerKind;
  mount_state: VaultMountState;
  /** Service-projected configured preference, separate from actual mounted drive_letter. */
  preferred_letter?: string | null;
  drive_letter: string | null;
}

/** Service-derived caller capability; never infer this from cached machine state. */
export interface VaultAccessCapabilities {
  can_manage_policy: boolean;
}

/** Owner-only edit fragment returned by the service. It is deliberately not
 * the machine's full durable policy: entries belonging to other owners never
 * cross into the renderer. */
export interface VaultOwnerPolicyEntry {
  entry: VaultAccessEntry;
  container_path_state: "available" | "unavailable";
  canonical_container_path?: string | null;
  /** Missing on an older service means deny in the UI until it is upgraded. */
  can_edit_policy?: boolean;
  can_remove_policy?: boolean;
}

export interface VaultOwnerPolicyFragment {
  schema_version: 1;
  policy_id: string | null;
  version: number;
  expected_previous_version: number;
  entries: VaultOwnerPolicyEntry[];
  remove_entry_ids?: string[];
}

export function vaultPolicyFromOwnerFragment(fragment: VaultOwnerPolicyFragment): VaultAccessPolicy | null {
  if (!fragment.policy_id) return null;
  return {
    schema_version: fragment.schema_version,
    policy_id: fragment.policy_id,
    version: fragment.version,
    expected_previous_version: fragment.expected_previous_version,
    entries: fragment.entries.map(({ entry, container_path_state, canonical_container_path, can_edit_policy, can_remove_policy }) => ({
      ...entry,
      container_path_state,
      canonical_container_path: canonical_container_path ?? null,
      can_edit_policy: can_edit_policy === true,
      can_remove_policy: can_remove_policy === true,
    })),
  };
}

/** Strip service-observation-only fields before applying the owner fragment.
 * The service independently resolves identity, canonical path, access and
 * mount state; the renderer cannot feed those observations back as authority. */
export function vaultOwnerFragmentFromPolicy(policy: VaultAccessPolicy, savedPolicy?: VaultAccessPolicy | null): VaultOwnerPolicyFragment {
  return {
    schema_version: policy.schema_version,
    policy_id: policy.policy_id,
    version: policy.version,
    expected_previous_version: policy.expected_previous_version,
    remove_entry_ids: savedPolicy?.entries.filter(saved => !policy.entries.some(entry => entry.id === saved.id)).map(entry => entry.id) ?? [],
    entries: policy.entries.map(entry => {
      const {
        container_path_state: _state,
        canonical_container_path: _canonical,
        can_edit_policy: _canEdit,
        can_remove_policy: _canRemove,
        ...requestEntry
      } = entry;
      // The shared fragment retains this required field for response-shape
      // compatibility. The service ignores it on writes and independently
      // validates/attests both saved and newly selected container paths.
      return { entry: requestEntry, container_path_state: "available" };
    }),
  };
}

/** A service-discovered Windows account that may become a Vault owner. */
export interface VaultOwnerPrincipal {
  sid: string;
  display_name: string;
  /** Derived by the service from the live local Administrators group. */
  is_local_administrator: boolean;
}

/** The service owns both the caller SID and the safe owner-picker options. */
export interface VaultOwnerPrincipalList {
  current_caller_sid: string;
  principals: VaultOwnerPrincipal[];
}

/** Never fall back to a draft or stale path. A saved unavailable path is
 * intentionally rendered as this exact bounded status. */
export function vaultCanonicalPathDisplay(entry: Pick<VaultAccessEntry, "container_path" | "container_path_state" | "canonical_container_path">): string {
  if (entry.container_path_state === "unavailable") return "Path unavailable";
  if (entry.container_path_state === "available") {
    // The broker returns Windows' extended-length canonical path (\\\\?\\C:\\...),
    // which is correct for native file operations but not the human display
    // contract. Strip only that prefix before validating the drive path.
    const canonical = entry.canonical_container_path;
    const displayCanonical = canonical?.startsWith("\\\\?\\")
      ? canonical.slice(4)
      : canonical;
    return displayCanonical?.match(/^[a-z]:\\/i)
      ? displayCanonical
      : "Path unavailable";
  }
  return entry.container_path;
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
  return result.reason
    ? VAULT_MOUNT_REASON_LABELS[result.reason]
    : "Mount request could not be completed";
}

export function newVaultEntry(kind: "shared" | "private" = "private"): VaultAccessEntry {
  const id = typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : fallbackId("vault");
  const shared = kind === "shared";
  return {
    id,
    label: shared ? "Shared vault" : "Personal vault",
    container_path: "",
    container_kind: "standard",
    // Never guess that this machine has an enabled account named
    // "Administrator". On a renamed, domain-joined, or different PC that
    // name may not resolve at all, which used to make a new policy fail only
    // after it reached the SYSTEM service. The administrator must choose a
    // current Windows user or group from the directory instead.
    owner_account: "",
    // A shared presentation changes only where Windows exposes the mounted
    // drive. It must not silently grant a generic local account write access.
    access_pattern: shared ? "shared-write" : "private",
    syncthing_opt_in: false,
    grants: [{ principal_name: "", access: "write" }],
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
      const { volume_kind: _legacyVolumeKind, access_pattern, ...normalized } = legacy;
      return {
        ...normalized,
        container_kind: legacy.container_kind ?? legacy.volume_kind ?? "standard",
        ...(access_pattern === "private" || access_pattern === "shared-read" || access_pattern === "shared-write"
          ? { access_pattern }
          : {}),
      };
    }),
  };
}

export function removeVaultEntryDraft(
  policy: VaultAccessPolicy,
  entryId: string,
  hasPersistedBase: boolean,
): VaultAccessPolicy | null {
  const next = { ...policy, entries: policy.entries.filter(entry => entry.id !== entryId) };
  return next.entries.length === 0 && !hasPersistedBase ? null : next;
}

export function nextVaultAccessPolicy(policy: VaultAccessPolicy): VaultAccessPolicy {
  return {
    ...policy,
    expected_previous_version: policy.version,
    version: policy.version + 1,
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
  return null;
}

let fallbackSequence = 0;

function fallbackId(prefix: string): string {
  fallbackSequence = (fallbackSequence + 1) % Number.MAX_SAFE_INTEGER;
  return `${prefix}-${Date.now().toString(36)}-${fallbackSequence.toString(36)}`;
}
