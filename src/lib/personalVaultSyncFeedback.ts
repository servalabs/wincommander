export const RECOVERED_SYNC_SHARING_GUIDANCE = "Share this new folder with your phone in Syncthing and accept it there. If the phone says the folder location is already in use, remove its old Syncthing folder entry while keeping the files, then accept the new share.";

export function personalVaultSyncSetupMessage(path: string, pairingRequired = false): string {
  if (pairingRequired) return `Sync setup for ${path} is ready. ${RECOVERED_SYNC_SHARING_GUIDANCE}`;
  return `Sync is configured for ${path}. Syncthing keeps it available while this personal Vault is mounted. Connect your other device in Syncthing to exchange files.`;
}

/** Translate bounded service outcomes without exposing private helper diagnostics. */
export function personalVaultSyncError(error: unknown): string {
  const detail = error instanceof Error ? error.message.toLowerCase() : typeof error === "string" ? error.toLowerCase() : "";
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
