// SPDX-License-Identifier: AGPL-3.0-or-later
import type { VaultCloudBackupConfiguration, VaultCloudBackupReceipt, VaultCloudBackupResult } from "@/types/generated/ipc";

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
  cloud_backup_remote_missing: "The backup folder was not found in Proton Drive. Check the destination or run a new backup first.",
  cloud_backup_restore_mismatch: "The restored files did not match the reported download. Restore has not been confirmed.",
  cloud_backup_verification_mismatch: "The checked file contents do not match Proton Drive. Backup has not been confirmed.",
  cloud_backup_verification_changed: "Files changed during the backup check. Wait for backup to finish, then verify again.",
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
  if (result.running) return result.last_run?.action === "restore" ? "Restoring files…"
    : result.last_run?.action === "verify" ? "Checking files in Proton Drive…"
      : result.last_run?.action === "start" ? "Backing up…" : "Backup operation running…";
  if (!result.engine_ready) return "Set up the backup tool";
  if (!result.connected) return "Proton Drive needs connecting";
  if (!result.configured) return "Choose what to back up";
  if (result.watching) return "Watching for changes — automatic backup enabled";
  return result.automatic ? "Automatic backup is paused" : "Manual backup enabled";
}

export function cloudBackupRunSummary(run: VaultCloudBackupReceipt) {
  const action = run.action;
  const name = action === "start" ? "Backup" : action === "verify" ? "Backup check" : action === "restore" ? "Restore" : "Operation";
  const finishedAt = typeof run.finished_at === "string" && /^\d{4}-\d{2}-\d{2}T/.test(run.finished_at)
    && Number.isFinite(Date.parse(run.finished_at)) ? run.finished_at : null;
  const safeRemote = run.remote_folder_name && !validateCloudBackupDraft({ whole_vault: true,
    source_relative_paths: [], remote_folder_name: run.remote_folder_name, automatic: false });
  const destination = action === "restore" ? run.restore_relative_path && validCloudBackupPath(run.restore_relative_path)
    ? `This Vault / ${run.restore_relative_path}` : null
    : safeRemote ? `Proton Drive / WinCommander Vault Backups / ${run.remote_folder_name}` : null;
  const summary = (title: string, detail: string, tone: "success" | "warning" | "neutral") =>
    ({ title, detail, tone, destination, finishedAt });
  if (!action) return summary("Previous result needs a new check", "Run a new backup, check or restore to confirm its result.", "neutral");
  if (run.outcome === "partial" || run.outcome === "failed" || run.outcome === "interrupted") {
    return summary(`${name} ${run.outcome === "partial" ? "partly finished" : run.outcome === "interrupted" ? "interrupted" : "failed"}`,
      "Completion has not been confirmed.", "warning");
  }
  if (run.outcome !== "complete" || !finishedAt || run.failed_items !== 0) {
    return summary(`${name} not confirmed yet`, "Wait for a completed result or refresh its status.", "neutral");
  }
  const checked = Number.isSafeInteger(run.checked_files) && run.checked_files > 0 && run.checked_files <= 1_000_000;
  const count = `${run.checked_files} sampled file${run.checked_files === 1 ? "" : "s"}`;
  const match = run.checked_files === 1 ? "matches" : "match";
  if (action === "restore") return run.confirmation === "restored_sample_matched" && checked && destination
    ? summary("Files restored and checked", `${count} ${match} Proton Drive.`, "success")
    : summary("Download finished; restore not checked", "No restored-file content check was recorded.", "warning");
  if (run.confirmation === "remote_sample_matched" && checked && safeRemote) {
    return summary(action === "verify" ? "Backup check passed" : "Backup checked in Proton Drive",
      `${count} ${match} your Vault.`, "success");
  }
  return action === "verify" ? summary("No files verified", "No file content check was recorded.", "warning")
    : summary("Upload finished; cloud contents not checked", "Run Verify backup to check the cloud copy.", "warning");
}

export function cloudBackupRecordedRuns(result: VaultCloudBackupResult) {
  const recorded = { backup: result.last_backup ?? null, verification: result.last_verification ?? null,
    restore: result.last_restore ?? null, previous: null as VaultCloudBackupReceipt | null };
  const latest = result.last_run;
  const key = latest?.action === "start" ? "backup" : latest?.action === "verify" ? "verification"
    : latest?.action === "restore" ? "restore" : null;
  if (latest && key) {
    const prior = recorded[key];
    if (!prior || latest.run_id === prior.run_id || Date.parse(latest.started_at) >= Date.parse(prior.started_at)) recorded[key] = latest;
  } else if (latest && !recorded.backup && !recorded.verification && !recorded.restore) recorded.previous = latest;
  return recorded;
}
