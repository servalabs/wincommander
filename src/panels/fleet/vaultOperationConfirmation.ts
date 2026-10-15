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
const containerPath = (value: string) => value.trim().replace(/^\\\\\?\\/, "").replaceAll("/", "\\").toLowerCase();
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
    && saved.container_path_state !== "unavailable"
    && containerPath(saved.container_path) === containerPath(entry.container_path)
    && Boolean(saved.syncthing_opt_in) === Boolean(entry.syncthing_opt_in)
    && (entry.primary_owner_sid ? saved.primary_owner_sid === entry.primary_owner_sid : principalName(saved.owner_account) === principalName(entry.owner_account))
    && grantIntent(saved.grants) === grantIntent(entry.grants)
    && saved.container_kind === entry.container_kind
    && saved.access_pattern === entry.access_pattern
    && saved.mount.presentation === entry.mount.presentation
    && (saved.mount.preferred_letter ?? "").toUpperCase() === (entry.mount.preferred_letter ?? "").toUpperCase()));
}

export function isUncertainVaultPolicySave(cause: unknown): boolean {
  const detail = (cause instanceof Error ? cause.message : String(cause ?? "")).toLowerCase();
  if (detail.includes("vault_policy_commit_unconfirmed")) return true;
  if (detail.includes("service rejected request:")) return false;
  return /vault_operation_unconfirmed|service operation did not confirm|service reply (?:timed out|read failed|signature check failed|did not match)|service request write (?:failed|timed out)|service returned an unexpected reply/.test(detail);
}

export async function recoverVaultPolicySave(
  expected: VaultAccessPolicy, removedIds: readonly string[], read: () => Promise<VaultAccessPolicy | null>,
  wait: (milliseconds: number) => Promise<void> = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds)),
): Promise<{ confirmed: boolean; policy: VaultAccessPolicy | null }> {
  // A missing caller projection can also mean unavailable authorization state.
  // Only a terminal save receipt can prove deletion; visibility loss is not revocation.
  if (removedIds.length || !expected.entries.length) return { confirmed: false, policy: null };
  for (let attempt = 0; attempt < 3; attempt++) {
    if (attempt) await wait(attempt * 500);
    try {
      const observed = await read();
      if (vaultPolicyRevisionConfirmed(expected, observed, removedIds)) return { confirmed: true, policy: observed };
    } catch { /* A failed observation grants no authority to repeat a save. */ }
  }
  return { confirmed: false, policy: null };
}

/** Report an actual ownership transfer before falling back to a generic edit conflict. */
export function vaultDraftConflictReason(draft: VaultAccessPolicy, base: VaultAccessPolicy | null, latest: VaultAccessPolicy | null, callerSid: string | null): string {
  if (callerSid && latest?.entries.some(entry => entry.mount.presentation === "per-user"
    && entry.primary_owner_sid && entry.primary_owner_sid !== callerSid
    && base?.entries.some(previous => previous.id === entry.id && previous.primary_owner_sid === callerSid)
    && draft.entries.some(edited => edited.id === entry.id))) return "vault_owner_required";
  return "Vault policy version conflict";
}
