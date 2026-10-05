// SPDX-License-Identifier: AGPL-3.0-or-later
import { vaultSyncRecoveryChoice, type PersonalVaultSyncEnrollment, type VaultSyncRecoveryChoice } from "./vaultSyncRecovery";
export type VaultSyncWarning = "stopped" | "unavailable" | "recovery_required";

export interface VaultSyncNotice {
  drive: string;
  warning: VaultSyncWarning;
  recovery?: VaultSyncRecoveryChoice;
}

export const VAULT_SYNC_WARNING_EVENT = "vault-sync-warning";

export function vaultSyncNotice(drive: unknown, warning: unknown): VaultSyncNotice | null {
  if (typeof drive !== "string" || !/^[A-Za-z]:?$/.test(drive)) return null;
  if (warning !== "stopped" && warning !== "unavailable" && warning !== "recovery_required") return null;
  return { drive: `${drive[0].toUpperCase()}:`, warning };
}

export function notifyVaultSyncRecovery(drive: string, internalDrive: number, enrollment: PersonalVaultSyncEnrollment, target: EventTarget = window): void {
  const notice = vaultSyncNotice(drive, "recovery_required");
  if (!notice) return;
  const recovery = vaultSyncRecoveryChoice(internalDrive, enrollment.recovery_roots);
  target.dispatchEvent(new CustomEvent(VAULT_SYNC_WARNING_EVENT, { detail: { ...notice, ...(recovery ? { recovery } : {}) } }));
}

export function notifyVaultSyncWarning(drive: unknown, warning: unknown, target: EventTarget = window): void {
  const notice = vaultSyncNotice(drive, warning);
  if (notice) target.dispatchEvent(new CustomEvent(VAULT_SYNC_WARNING_EVENT, { detail: notice }));
}

export function vaultSyncWarningMessage(notice: VaultSyncNotice): string {
  if (notice.warning === "recovery_required") return `${notice.drive} The Vault is mounted. One or more sync folders need your recovery decision and remain paused. Your other sync folders can continue normally.`;
  const syncIssue = notice.warning === "stopped"
    ? "Syncthing is still stopped after an automatic rescan."
    : "WinCommander could not confirm Syncthing's status after checking it.";
  return `${notice.drive} The file container mounted successfully and your files remain accessible. ${syncIssue} Keep the container mounted and open Syncthing to check its folder error. Sync has not been confirmed; the container was not dismounted.`;
}

/** A mount receipt may carry a sync warning without becoming a failed mount. */
export function notifyPolicyMountSyncWarning(result: {
  state: string;
  drive_letter?: string | null;
  sync_warning?: VaultSyncWarning | null;
}, target: EventTarget = window): void {
  if (result.state === "mounted") notifyVaultSyncWarning(result.drive_letter, result.sync_warning, target);
}

export function notifyPersonalMountSyncWarning(result: {
  success: boolean;
  data?: { status: string; drive: string; syncWarning?: VaultSyncWarning | null } | null;
}, target: EventTarget = window): void {
  if (result.success && result.data?.status === "mounted") {
    notifyVaultSyncWarning(result.data.drive, result.data.syncWarning, target);
  }
}
