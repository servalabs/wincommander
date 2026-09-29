import { VAULT_MOUNT_REASONS, vaultMountResultLabel } from "@/panels/fleet/vaultAccessTypes";
import type { EncryptionStatus, MountVolumeResult } from "@/hooks/useBackend";

/** Call only after the mount receipt and Windows drive verification succeeded. */
export function confirmedMountObservationError(mount: Pick<MountVolumeResult, "drive" | "internalDrive">, observed: EncryptionStatus | null | undefined): string | null {
  if (!observed || !Array.isArray(observed.volumes)) return "vault_confirmed_mount_list_unavailable";
  return observed.volumes.some(volume => volume.letter === mount.drive && volume.internalDrive === mount.internalDrive && volume.accessible !== false)
    ? null : "vault_confirmed_mount_not_in_list";
}

export function isAuthorizedBulkDismountReceipt(value: unknown): boolean {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const receipt = value as Record<string, unknown>;
  return receipt.status === "authorized_dismounted" && receipt.state === "unmounted" && receipt.scope === "authorized"
    && typeof receipt.dismounted === "number" && Number.isSafeInteger(receipt.dismounted)
    && receipt.dismounted >= 0 && receipt.dismounted <= 26;
}

/** Show only known failure categories, never raw service transport or credential data. */
export function vaultOperationError(error: unknown, operation: "mount" | "dismount" | "open" = "mount"): string {
  const detail = (error instanceof Error ? error.message : typeof error === "string" ? error : "").toLowerCase();
  if (detail.includes("vault_mount_options_timeout")) return "The Vault service did not finish checking the drive list. No mount was started. Wait for any current operation to finish, then refresh the free letters.";
  if (detail.includes("vault_request_timeout") || detail.includes("pro response timeout")) return "The Vault service did not return an operation result in time. The operation may still be running. Do not submit it again; refresh Secure Storage to check the drive's status first.";
  if (detail.includes("vault_operation_unconfirmed")) return "The connection to the Vault service was interrupted before its result arrived. The operation may have completed. Refresh Secure Storage to check the drive's status before trying again.";
  if (detail.includes("vault_service_busy") || detail.includes("all pipe instances are busy")) return "The Vault service is busy and could not accept this request. Wait for the current operation to finish, then refresh Secure Storage before retrying.";
  if (detail.includes("vault_runtime_update_required")) return "The installed Pro component is incompatible with this WinCommander build. Install a matching Pro update from License / Pro. If no compatible update is available, the matching release must be published first; reinstalling the older Pro will not fix it. No mount or dismount was started.";
  if (detail.includes("vault_service_personal_status_invalid")) return "The installed WinCommander app, Pro component and Vault service could not exchange a compatible status result. Update or repair them together, then refresh the mounted-volume status. The current mount or dismount state could not be verified.";
  if (detail.includes("vault_confirmed_mount_list_unavailable")) return "The mount and Windows drive access were confirmed, but the mounted-volume list could not be refreshed. This does not mean mounting failed. Refresh status before using the drive; do not mount it again.";
  if (detail.includes("vault_confirmed_mount_not_in_list")) return "The mount and Windows drive access were confirmed, but the latest volume list no longer shows that same accessible drive. Its state may have changed. Refresh status before using or mounting it again.";
  const partial = detail.match(/(?:^|\s)vault_bulk_dismount_partial:([a-z_]+):(\d+):(\d+)(?:$|\s)/);
  if (partial && VAULT_MOUNT_REASONS.includes(partial[1] as typeof VAULT_MOUNT_REASONS[number])) {
    const dismounted = Number(partial[2]);
    const blocked = Number(partial[3]);
    if (Number.isSafeInteger(dismounted) && Number.isSafeInteger(blocked)) return `${dismounted} encrypted volume(s) dismounted; ${blocked} not confirmed dismounted. ${vaultOperationError(`vault_${partial[1]}`, "dismount")}`;
  }
  if (detail.includes("pro_not_installed")) return "The Pro module is not installed. Open License / Pro and install it before mounting. Activating a licence alone does not install the encryption engine.";
  if (detail.includes("vault_owner_required")) return "Only the primary owner can edit this private Vault. Ask its owner to make changes. An administrator may remove the policy only while the Vault is unmounted; this does not unlock its contents.";
  if (detail.includes("vault_mount_readback_unconfirmed")) return "The service has not confirmed that this Vault is mounted for your account. Refresh its status before using the drive. No successful mount was reported.";
  if (detail.includes("vault_dismount_readback_unconfirmed")) return "The service has not confirmed that this Vault was dismounted. Refresh its status before removing the device or changing its policy.";
  const reason = VAULT_MOUNT_REASONS.find(value => new RegExp(`(?:^|[^a-z_])(?:vault_)?${value}(?:$|[^a-z_])`).test(detail));
  if (reason) {
    const label = vaultMountResultLabel({ entry_id: "", state: "failed", presentation: null, drive_letter: null, reason });
    if (reason === "engine_unlock_failed") return `${label}. Check the password, PIM and keyfiles, then try again.`;
    if (reason === "engine_drive_letter_unavailable") return `${label} or reserved by another Vault. Refresh and select a free letter.`;
    if (reason === "caller_access_denied") return `${label}. Review its Windows permissions in Secure Storage before retrying.`;
    return `${label}.`;
  }
  if (detail.includes("vault_policy_managed")) return "This container has saved Vault permissions. Mount it from Saved Fleet Vaults or Fleet → Vault permissions.";
  if (detail.includes("already mounted")) return "This container is already mounted. Open it from the mounted volumes list in Secure Storage.";
  if (detail.includes("letter") && (detail.includes("in use") || detail.includes("reserved") || detail.includes("unavailable"))) return "That drive letter is in use or reserved by another Vault. Refresh and select a free letter.";
  if (detail.includes("access is denied") || detail.includes("access denied")) return "Windows denied access for this account. Review the container's Windows permissions; administrator approval does not replace Vault ownership.";
  if (detail.includes("elevation") || detail.includes("administrator privileges")) return "This operation needs administrator approval. Use an administrator account or approve the Windows permission prompt, then retry.";
  if (operation === "open") return "WinCommander could not open this drive for your Windows account. Refresh its mount status and try again.";
  if (operation === "dismount") return "The Vault could not be dismounted. Close files using the drive, refresh its status, and try again.";
  return "The Vault could not be mounted. Check the selected container and refresh its status before retrying. No successful mount was confirmed.";
}

export function selectableDriveLetters(available: readonly string[], reserved: readonly string[] = []): string[] {
  const taken = new Set(reserved.map(letter => letter.replace(/:$/, "").toUpperCase()));
  return [...new Set(available.map(letter => letter.replace(/:$/, "").toUpperCase()))]
    .filter(letter => /^[A-Z]$/.test(letter) && !taken.has(letter));
}
