export function vaultCreationFailureDetail(cause: unknown): string {
  const message = cause instanceof Error ? cause.message : String(cause ?? "");
  const normalized = message.toLowerCase();
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
  if (normalized.includes("vault_creation_verification_failed")) {
    return "WinCommander could not verify the newly created Vault. Do not use the selected file until Secure Storage confirms it.";
  }
  return "WinCommander could not create this Vault. No successful Vault was confirmed; try again and send the diagnostic reference if it repeats.";
}
