// SPDX-License-Identifier: AGPL-3.0-or-later
import type { VaultCloudBackupConfiguration, VaultCloudBackupResult } from "@/types/generated/ipc";

const RESERVED = new Set([".wincommander", "system volume information", "$recycle.bin"]);
export function validCloudBackupPath(path: string): boolean {
  const parts = path.split(/[\\/]/);
  return path.length > 0 && new TextEncoder().encode(path).length <= 240
    && !/[\x00-\x1f\x7f-\x9f:*?"<>|]/.test(path)
    && parts.every(part => Boolean(part) && part !== "." && part !== ".." && !/[. ]$/.test(part))
    && !RESERVED.has(parts[0]!.toLowerCase());
}
export function validateCloudBackupDraft(configuration: VaultCloudBackupConfiguration): string | null {
  const name = configuration.remote_folder_name;
  if (!name || new TextEncoder().encode(name).length > 128 || name.trim() !== name
    || name === "." || name === ".." || /[. ]$/.test(name) || /[\x00-\x1f\x7f-\x9f\\/:*?"<>|]/.test(name)) {
    return "Give the Proton backup folder a name of up to 128 bytes without slashes or special filename characters.";
  }
  const paths = configuration.source_relative_paths;
  if (configuration.whole_vault) return paths.length === 0 ? null : "Whole Vault cannot also contain selected folders.";
  if (paths.length === 0 || paths.length > 32 || paths.some(path => !validCloudBackupPath(path))) {
    return "Choose 1–32 paths inside this Vault, for example Documents. Backup settings and Windows system folders cannot be selected.";
  }
  const normalized = paths.map(path => path.replaceAll("/", "\\").toLowerCase());
  if (normalized.some((path, index) => normalized.slice(0, index).some(other => path === other
    || path.startsWith(`${other}\\`) || other.startsWith(`${path}\\`)))) {
    return "Choose separate folders that do not overlap or repeat.";
  }
  return null;
}
export function acceptsCloudBackupReceipt(current: string | null, incoming: unknown): incoming is string {
  return typeof incoming === "string" && /^[a-f0-9]{64}$/.test(incoming) && (!current || current === incoming);
}
const MESSAGES: Record<string, string> = {
  cloud_backup_not_permitted: "This Windows account cannot back up this Vault. Ask its owner to review your access.",
  cloud_backup_policy_disabled: "Cloud backup is not enabled for this Vault by its owner. Enable it in Vault permissions and save the policy first.",
  cloud_backup_read_only_access: "Cloud backup requires write access to save its protected settings inside this Vault.",
  cloud_backup_acl_unavailable: "This Vault cannot protect the saved Proton session with Windows permissions. Use an NTFS Vault.",
  cloud_backup_not_signed_in: "Reconnect Proton Drive to resume backup. Opening the Vault does not normally require another sign-in.",
  cloud_backup_sign_in_timeout: "Proton sign-in did not finish in time. Choose Connect Proton Drive to try again.",
  cloud_backup_mount_changed: "This Vault was closed or its mounted identity changed. Close this window and reopen Cloud backup for the current Vault.",
  cloud_backup_source_unsafe: "A selected source is unavailable or contains a link, junction or unsupported path. Review the selected folders.",
  cloud_backup_source_overlap: "Backup sources overlap with protected settings or another configured folder. Choose separate source folders.",
  cloud_backup_already_running: "A backup is already active for this Vault. Refresh to see its current state.",
  cloud_backup_remote_conflict: "The remote folder could not be safely adopted. Choose a new backup folder name.",
  cloud_backup_no_space: "There is not enough free storage to complete this operation. Check the Vault and Proton Drive storage.",
  cloud_backup_interrupted: "Backup was interrupted. Files already uploaded remain in Proton Drive; the next pass checks the remaining files.",
  cloud_backup_engine_download_failed: "The backup tool could not be downloaded. Check the connection and retry setup.",
  cloud_backup_engine_integrity_failed: "The backup tool failed its integrity check. No upload was started.",
  cloud_backup_engine_version_mismatch: "The installed backup tool has an unsupported version. Retry setup.",
  cloud_backup_cli_output_unrecognized: "The backup tool returned an unrecognized result. Completion has not been confirmed.",
};
export function cloudBackupError(cause: unknown): string {
  const detail = cause instanceof Error ? cause.message : String(cause);
  const code = detail.match(/cloud_backup_[a-z_]+/)?.[0];
  return code && MESSAGES[code] ? MESSAGES[code]
    : "The backup result could not be confirmed. Refresh to check its state before trying again.";
}
export function cloudBackupStateLabel(result: VaultCloudBackupResult): string {
  if (result.reason_code === "cloud_backup_sign_in_pending") return "Complete Proton sign-in in your browser. Keep this Vault open.";
  if (result.running) return "Uploading — completion not yet confirmed";
  if (!result.engine_ready) return "Set up the backup tool";
  if (!result.connected) return "Proton Drive needs connecting";
  if (!result.configured) return "Choose what to back up";
  if (result.watching) return "Watching for changes — automatic backup enabled";
  return result.automatic ? "Automatic backup is paused" : "Manual backup enabled";
}
