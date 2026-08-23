import type { VaultAuthorizedEntry, VaultMountEntryResult } from "./vaultAccessTypes";

/**
 * The mount broker returns the authoritative lifecycle result for one entry.
 * Patch only that caller-filtered row; policy or authorization changes still
 * require a full service refresh.
 */
export function patchAuthorizedEntriesFromMountResult(
  entries: VaultAuthorizedEntry[],
  result: VaultMountEntryResult,
): VaultAuthorizedEntry[] {
  return entries.map(entry => entry.entry_id === result.entry_id
    ? {
      ...entry,
      mount_state: result.state,
      drive_letter: result.state === "mounted" ? result.drive_letter : null,
    }
    : entry);
}
