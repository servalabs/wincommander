import type { VaultAccessPolicy } from "./vaultAccessTypes";

const VAULT_ACCESS_DRAFT_KEY = "wincommander.vault-access-draft.v1";

export interface VaultDraftStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

function isVaultAccessPolicy(value: unknown): value is VaultAccessPolicy {
  if (!value || typeof value !== "object") return false;
  const policy = value as Partial<VaultAccessPolicy>;
  if (
    policy.schema_version !== 1
    || typeof policy.policy_id !== "string"
    || typeof policy.version !== "number"
    || typeof policy.expected_previous_version !== "number"
    || !Array.isArray(policy.entries)
  ) return false;

  return policy.entries.every(entry => (
    !!entry
    && typeof entry.id === "string"
    && typeof entry.label === "string"
    && typeof entry.container_path === "string"
    && (entry.container_identity === undefined || entry.container_identity === null || typeof entry.container_identity === "string")
    && typeof entry.owner_account === "string"
    && Array.isArray(entry.grants)
    && entry.grants.every(grant => (
      !!grant
      && typeof grant.principal_name === "string"
      && (grant.access === "read" || grant.access === "write")
    ))
    && !!entry.mount
    && (entry.mount.presentation === "machine" || entry.mount.presentation === "per-user")
    && (entry.mount.preferred_letter === undefined || entry.mount.preferred_letter === null || typeof entry.mount.preferred_letter === "string")
  ));
}

function browserStorage(): VaultDraftStorage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/**
 * Renderer drafts contain paths and account names only. They never contain a
 * mount secret and never count as applied policy; the SYSTEM service still
 * resolves and validates every field when the administrator applies it.
 */
export function readVaultAccessDraft(storage: VaultDraftStorage | null = browserStorage()): VaultAccessPolicy | null {
  if (!storage) return null;
  try {
    const raw = storage.getItem(VAULT_ACCESS_DRAFT_KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    return isVaultAccessPolicy(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

export function writeVaultAccessDraft(policy: VaultAccessPolicy, storage: VaultDraftStorage | null = browserStorage()): void {
  if (!storage) return;
  try {
    storage.setItem(VAULT_ACCESS_DRAFT_KEY, JSON.stringify(policy));
  } catch {
    // A blocked/full browser store must not prevent editing or applying.
  }
}

export function clearVaultAccessDraft(storage: VaultDraftStorage | null = browserStorage()): void {
  if (!storage) return;
  try {
    storage.removeItem(VAULT_ACCESS_DRAFT_KEY);
  } catch {
    // Applying remains service-owned even when local cleanup is blocked.
  }
}
