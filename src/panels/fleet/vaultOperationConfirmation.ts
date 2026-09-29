import type { VaultAccessPolicy, VaultAuthorizedEntry, VaultGrantInput, VaultMountEntryResult } from "./vaultAccessTypes";

export function vaultMountResultConfirmed(result: VaultMountEntryResult, entries: readonly VaultAuthorizedEntry[]): boolean {
  const observed = entries.find(entry => entry.entry_id === result.entry_id);
  if (!observed || observed.mount_state !== result.state) return false;
  if (result.state === "unmounted") return true;
  if (result.state !== "mounted") return false;
  if (observed.presentation !== result.presentation) return false;
  const letter = (value: string | null) => value?.replace(/:$/, "").toUpperCase() ?? null;
  const expectedLetter = letter(result.drive_letter);
  return !!expectedLetter && /^[A-Z]$/.test(expectedLetter) && letter(observed.drive_letter) === expectedLetter;
}

const principalName = (value: string) => value.trim().toLowerCase();
// The service preserves requested grant names in its saved policy; resolved
// SID/group ACL plans are separate, service-only fields.
const grantIntent = (grants: readonly VaultGrantInput[]) => JSON.stringify(grants
  .map(grant => [principalName(grant.principal_name), grant.access]).sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b))));

export function vaultPolicyRevisionConfirmed(expected: VaultAccessPolicy, observed: VaultAccessPolicy | null, removedIds: readonly string[]): boolean {
  if (removedIds.some(id => observed?.entries.some(entry => entry.id === id))) return false;
  if (expected.entries.length === 0) return !observed || observed.entries.length === 0;
  if (!observed || observed.policy_id !== expected.policy_id || observed.version < expected.version) return false;
  return expected.entries.every(entry => observed.entries.some(saved => saved.id === entry.id
    && saved.label === entry.label
    && (entry.primary_owner_sid ? saved.primary_owner_sid === entry.primary_owner_sid : principalName(saved.owner_account) === principalName(entry.owner_account))
    && grantIntent(saved.grants) === grantIntent(entry.grants)
    && saved.container_kind === entry.container_kind
    && saved.access_pattern === entry.access_pattern
    && saved.mount.presentation === entry.mount.presentation
    && (saved.mount.preferred_letter ?? "").toUpperCase() === (entry.mount.preferred_letter ?? "").toUpperCase()));
}

/** Report an actual ownership transfer before falling back to a generic edit conflict. */
export function vaultDraftConflictReason(draft: VaultAccessPolicy, base: VaultAccessPolicy | null, latest: VaultAccessPolicy | null, callerSid: string | null): string {
  if (callerSid && latest?.entries.some(entry => entry.mount.presentation === "per-user"
    && entry.primary_owner_sid && entry.primary_owner_sid !== callerSid
    && base?.entries.some(previous => previous.id === entry.id && previous.primary_owner_sid === callerSid)
    && draft.entries.some(edited => edited.id === entry.id))) return "vault_owner_required";
  return "Vault policy version conflict";
}
