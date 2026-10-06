export const RECOVERED_SYNC_SHARING_GUIDANCE = "Share this new folder with your phone in Syncthing and accept it there. If the phone says the folder location is already in use, remove its old Syncthing folder entry while keeping the files, then accept the new share.";

export type VaultSyncFolderValidation =
  | { ok: true; folders: string[] }
  | { ok: false; message: string };

/**
 * Normalise a short, Vault-relative folder name before it crosses the IPC
 * boundary. The service remains authoritative; this gives the person a clear
 * explanation before a batch can create partly configured folders.
 */
export function validatePersonalVaultSyncFolders(values: readonly string[]): VaultSyncFolderValidation {
  if (!Array.isArray(values) || values.length === 0 || values.length > 32) {
    return { ok: false, message: "Add between one and 32 separate folders inside this Vault." };
  }
  const folders: string[] = [];
  for (const value of values) {
    const folder = value.trim().replaceAll("/", "\\");
    const parts = folder.split("\\");
    if (!folder || folder.length > 240 || parts.some((part: string) => !part || part === "." || part === ".." || /[:\x00-\x1f]/.test(part))) {
      return { ok: false, message: "Use a folder inside the Vault, such as Phone\\Camera. Do not use a drive letter, the Vault root, or .." };
    }
    folders.push(folder);
  }
  const keys = folders.map(folder => folder.toLocaleLowerCase());
  if (new Set(keys).size !== keys.length) {
    return { ok: false, message: "Each sync folder must be listed only once." };
  }
  for (let index = 0; index < keys.length; index += 1) {
    for (let other = index + 1; other < keys.length; other += 1) {
      if (keys[index].startsWith(`${keys[other]}\\`) || keys[other].startsWith(`${keys[index]}\\`)) {
        return { ok: false, message: "Sync folders cannot overlap. For example, use Phone\\Camera and Phone\\Documents, not Phone and Phone\\Camera." };
      }
    }
  }
  return { ok: true, folders };
}

export function personalVaultSyncSetupMessage(path: string, pairingRequired = false): string {
  if (pairingRequired) return `Sync setup for ${path} is ready. ${RECOVERED_SYNC_SHARING_GUIDANCE}`;
  return `Sync is configured for ${path}. Its folder pauses when this personal Vault is dismounted and resumes after it is mounted again; that does not stop Syncthing or other sync folders. Connect your other device in Syncthing to exchange files.`;
}

/** Translate bounded service outcomes without exposing private helper diagnostics. */
export function personalVaultSyncError(error: unknown): string {
  const detail = error instanceof Error ? error.message.toLowerCase() : typeof error === "string" ? error.toLowerCase() : "";
  if (detail.includes("vault_sync_mount_changed")) return "The Vault in this drive slot has changed. Close this dialog, refresh Secure Storage, and open sync settings for the intended Vault again.";
  if (detail.includes("vault_sync_label_update_failed")) return "The folder was enabled, but its name update could not be confirmed. Refresh the list and use Save name on that folder; you do not need to enable it again.";
  if (detail.includes("vault_syncthing_not_enabled")) return "Sync is off for this personal Vault. In Vault access, edit this Vault, turn on Syncthing, save the policy, then try again.";
  if (detail.includes("vault_not_authorized")) return "Sync is available only for a personal Vault owned and mounted by this Windows account. Shared Vaults cannot use it.";
  if (detail.includes("vault_mount_state_unknown")) return "WinCommander could not confirm that this Vault is still mounted for your account. Refresh Secure Storage, then try again.";
  if (detail.includes("vault_syncthing_install_failed")) return "Syncthing could not be installed or its download could not be verified. Check your Internet connection and available disk space, then enable sync again.";
  if (detail.includes("vault_syncthing_profile_unavailable")) return "Syncthing could not start using this account's saved configuration. Your sync identity has been preserved. Check Diagnostics for the startup failure.";
  if (detail.includes("vault_syncthing_root_conflict")) return "The saved sync folder conflicts with the selected folder or its current location. Check the folder in Syncthing before retrying; do not add a duplicate folder.";
  if (detail.includes("vault_broker_rejected")) return "The sync helper responded but could not complete this folder's setup. Check Diagnostics for the reported cause, then retry with the Vault mounted.";
  if (detail.includes("vault_broker_unavailable")) return "WinCommander could not start the sync helper for this account. Check that the matching Pro component is installed, then retry with the Vault mounted.";
  if (detail.includes("vault_request_timeout") || detail.includes("vault_operation_unconfirmed") || detail.includes("service operation did not confirm before its deadline")) return "Sync setup has not returned a result yet. Keep the Vault mounted and check its sync status before submitting another request.";
  return "Personal Vault sync could not be set up. Keep the Vault mounted and check Diagnostics for the cause.";
}
