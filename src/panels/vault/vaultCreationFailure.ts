export function vaultCreationFailureDetail(cause: unknown): string {
  const message = cause instanceof Error ? cause.message : String(cause ?? "");
  const normalized = message.toLowerCase();
  if (normalized.includes("encryption_driver_restart_required:")) {
    return "Save your work and restart Windows to activate the updated encryption driver, then try creating the Vault again. You can still dismount existing vaults. Restarting WinCommander alone will not activate the update.";
  }
  if (normalized.includes("vault_driver_unavailable")) {
    return "WinCommander's encrypted-volume driver could not be prepared by the installed service. Ask an administrator to repair WinCommander; no Vault was created.";
  }
  if (normalized.includes("vault_broker_unavailable")) {
    return "WinCommander's Vault helper is missing or unavailable. Ask an administrator to repair WinCommander; no Vault was created.";
  }
  if (normalized.includes("vault_session_unavailable")) {
    return "WinCommander needs an active signed-in Windows session to create a Vault. Sign in normally, then try again.";
  }
  if (normalized.includes("vault_container_not_writable")) {
    return "Windows cannot write to the chosen Vault location. Choose a folder owned by this Windows account, then try again.";
  }
  if (normalized.includes("vault_entitlement_denied")) {
    return "WinCommander could not verify the Pro licence for this Vault operation. Refresh the licence status, then try again.";
  }
  if (normalized.includes("vault_target_exists")) {
    return "A file or folder with that name already exists in the chosen location, so no Vault was created and nothing was changed. Choose a different name or location, then try again.";
  }
  if (normalized.includes("vault_path_reserved")) {
    return "The chosen file is already governed by Vault permissions, so no Vault was created. Choose a different file name, or manage that Vault from Secure Storage.";
  }
  if (normalized.includes("vault_owner_record_failed")) {
    return "WinCommander could not record ownership of the new Vault, so none was created. Choose a different location and try again; if it repeats, send the diagnostic reference. Diagnostic code: VLT.CREATE.OWNER_RECORD_FAILED.";
  }
  if (normalized.includes("vault_creation_verification_failed")) {
    return "WinCommander could not verify the newly created Vault. Do not use the selected file until Secure Storage confirms it.";
  }
  if (normalized.includes("vault_validation_failed")) {
    return "WinCommander rejected this request because the selected settings or partition no longer pass its safety checks. Go Back, refresh the partition selection, and try again. No Vault was created. Diagnostic code: VLT.CREATE.VALIDATION_FAILED.";
  }
  if (normalized.includes("vault_creation_preparation_failed")) {
    return "There is not enough free space to prepare this Vault. Free space of about twice the Vault size is needed on the Windows drive and on the destination drive if WinCommander is not running as an administrator, and about the Vault size otherwise. Free some space or choose a larger drive, then try again.";
  }
  if (normalized.includes("vault_container_too_large")) {
    return "The Vault is larger than the free space on the chosen drive, so no Vault was created. Choose a drive with more free space or a smaller size, then try again.";
  }
  if (normalized.includes("vault_hidden_too_large")) {
    return "The hidden volume is too large for the outer volume. Go Back and reduce the hidden volume size, or increase the total size, then try again. No Vault was created.";
  }
  if (normalized.includes("vault_format_files_open")) {
    return "Another program, often antivirus or file indexing, held the new Vault open while it was being prepared. Try again; if it repeats, pause that program for a few minutes or exclude the Vault folder from it, then try again.";
  }
  if (normalized.includes("vault_create_timeout")) {
    return "Creating this Vault took longer than the time WinCommander allows. Large Vaults, especially hidden-volume Vaults, can take a long time, and WinCommander now waits longer in proportion to the size. Try again and keep WinCommander open until it finishes.";
  }
  return "WinCommander could not create this Vault. No successful Vault was confirmed; try again and send the diagnostic reference if it repeats.";
}
