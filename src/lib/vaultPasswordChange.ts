import { cleanVaultPath, serviceDetail } from "./vaultRename";

/** The header a password opened: a decoy vault keeps its decoy password in the outer header and its real one in the hidden header. */
export type VaultPasswordHeader = "outer" | "hidden";

/** One header's credentials. */
export interface HeaderCredentials {
  oldPassword: string;
  newPassword: string;
  confirmPassword: string;
  oldPim: string;
  newPim: string;
  oldKeyfile: string;
  newKeyfile: string;
}

export interface VaultPasswordProbeResult {
  header: VaultPasswordHeader;
}

export interface VaultPasswordChangeResult {
  status: "password_changed";
  path: string;
  target: VaultPasswordHeader;
}

/** Which call failed decides the wording: a decoy follow-up runs after the real password already changed. */
export type PasswordChangeStage = "probe" | "change" | "decoy";

export const EMPTY_HEADER_CREDENTIALS: HeaderCredentials = {
  oldPassword: "",
  newPassword: "",
  confirmPassword: "",
  oldPim: "",
  newPim: "",
  oldKeyfile: "",
  newKeyfile: "",
};

const MAX_PIM = 2_147_468;
const PIM_PROBLEM = "A PIM must be a whole number from 1 to 2147468, or left blank.";
const validPim = (value: string) => !value || (/^\d+$/.test(value) && Number(value) >= 1 && Number(value) <= MAX_PIM);
const hasCurrent = (header: HeaderCredentials) => Boolean(header.oldPassword || header.oldKeyfile.trim());
const hasNew = (header: HeaderCredentials) => Boolean(header.newPassword || header.newKeyfile.trim());
const unchanged = (header: HeaderCredentials) => header.newPassword === header.oldPassword
  && header.newPim === header.oldPim && header.newKeyfile.trim() === header.oldKeyfile.trim();

/** Returns the first problem with the vault file and current password, or null when they can be checked. */
export function currentPasswordProblem(volumePath: string, header: HeaderCredentials): string | null {
  if (!volumePath.trim()) return "Choose the vault file.";
  if (!hasCurrent(header)) return "Enter the current password.";
  if (!validPim(header.oldPim)) return PIM_PROBLEM;
  return null;
}

/** Returns the first problem with a new password, or null when the change can be sent; `word` is "" or "decoy ". */
export function newPasswordProblem(header: HeaderCredentials, word: "" | "decoy " = ""): string | null {
  if (!hasCurrent(header)) return `Enter the current ${word}password.`;
  if (!hasNew(header)) return `Enter a new ${word}password.`;
  if (header.newPassword !== header.confirmPassword) return `The new ${word}passwords do not match.`;
  if (!validPim(header.oldPim) || !validPim(header.newPim)) return PIM_PROBLEM;
  if (unchanged(header)) return `The new ${word}password must be different from the current one.`;
  return null;
}

const keyfileList = (path: string) => JSON.stringify(path.trim() ? [path.trim()] : []);

/** Field names follow the Pro "Test-EncryptionVolumePassword" handler; lists travel as JSON text like the mount form's. */
export function buildPasswordProbeRequest(volumePath: string, header: HeaderCredentials): Record<string, string> {
  return {
    VolumePath: cleanVaultPath(volumePath),
    TargetKind: "file",
    Password: header.oldPassword,
    Keyfiles: keyfileList(header.oldKeyfile),
    ...(header.oldPim ? { Pim: header.oldPim } : {}),
  };
}

/** Field names follow the Pro "Set-EncryptionVolumePassword" handler; the engine re-checks the current password itself.
 * `provenReal` (decoy follow-up only) lets the engine refuse a new decoy equal to the real password it first re-proves. */
export function buildPasswordChangeRequest(volumePath: string, target: VaultPasswordHeader, header: HeaderCredentials, provenReal?: HeaderCredentials | null): Record<string, string> {
  return {
    VolumePath: cleanVaultPath(volumePath),
    TargetKind: "file",
    Target: target,
    OldPassword: header.oldPassword,
    NewPassword: header.newPassword,
    OldKeyfiles: keyfileList(header.oldKeyfile),
    NewKeyfiles: keyfileList(header.newKeyfile),
    ...(header.oldPim ? { OldPim: header.oldPim } : {}),
    ...(header.newPim ? { NewPim: header.newPim } : {}),
    ...(target === "outer" && provenReal ? {
      HiddenPassword: provenReal.oldPassword,
      HiddenKeyfiles: keyfileList(provenReal.oldKeyfile),
      ...(provenReal.oldPim ? { HiddenPim: provenReal.oldPim } : {}),
    } : {}),
  };
}

/** Accepts only the two contract values, so a malformed reply never opens a change form. */
export function readProbeHeader(data: unknown): VaultPasswordHeader | null {
  const header = (data as { header?: unknown } | null)?.header;
  return header === "outer" || header === "hidden" ? header : null;
}

export const PASSWORD_PROBE_BUSY = "Checking the password. This can take up to a minute; keep this window open.";

export const PASSWORD_CHANGE_BUSY = "Checking the current password and changing it. This can take up to a minute; keep this window open.";

export const PASSWORD_CHANGE_WARNING = "Header backups and copies of this container made before now still open with the old password.";

export const WRONG_PASSWORD = "Wrong password.";

export const WRONG_DECOY_PASSWORD = "Wrong password. The first password change was kept.";

/** The service noted a hidden part and the decoy password opened only the outer one. */
export const REAL_PASSWORD_REQUIRED = "Wrong password. Enter the real password for this vault.";

export const POLICY_MANAGED = "Only this vault's owner or an administrator can change its password.";

const errorText = (error: unknown) => (error instanceof Error ? error.message : typeof error === "string" ? error : "").toLowerCase();
const hasCode = (detail: string, code: string) => new RegExp(`(?:^|[^a-z_])(?:vault_)?${code}(?:$|[^a-z_])`).test(detail);

export function isPolicyManagedError(error: unknown): boolean {
  return hasCode(errorText(error), "vault_policy_managed");
}

/** Maps the stable service and engine codes to plain wording; never echoes raw error text. */
export function vaultPasswordChangeErrorMessage(error: unknown, stage: PasswordChangeStage): string {
  const detail = errorText(error);
  const has = (code: string) => hasCode(detail, code);
  if (has("vault_policy_managed")) return POLICY_MANAGED;
  if (has("real_password_required")) {
    return stage === "decoy" ? "The decoy password was not changed. The first password change was kept." : REAL_PASSWORD_REQUIRED;
  }
  if (has("wrong_password") || has("engine_unlock_failed")) return stage === "decoy" ? WRONG_DECOY_PASSWORD : WRONG_PASSWORD;
  // Outside the decoy follow-up a collision would tell a decoy-password holder something about the real password.
  if (stage === "decoy" && has("new_password_collides")) return "The new decoy password must differ from the real password.";
  if (has("weak_new_password")) return "The new password is too weak. Use a longer password with a mix of words, numbers and symbols. Nothing was changed.";
  if (has("volume_mounted") || has("already_mounted")) return "Dismount the vault first, then change its password. Nothing was changed.";
  if (has("mount_state_unknown")) return "WinCommander could not confirm that this vault is dismounted. Refresh Secure Storage, dismount it if it is listed, then try again.";
  if (has("vault_file_not_found")) return "WinCommander could not find the vault file. Check the path, then try again. Nothing was changed.";
  if (has("vault_access_denied")) return "Windows did not let this account open the vault file. Check that the file is not read-only and that this account can change it. Nothing was changed.";
  if (has("vault_file_in_use")) return "Another program is using the vault file. Close it, then try again. Nothing was changed.";
  if (has("engine_failed")) return "The encryption engine could not save the new password, so the original was kept. Nothing changed.";
  if (has("vault_request_timeout") || has("vault_operation_unconfirmed") || detail.includes("pro response timeout")) {
    return stage === "probe"
      ? "WinCommander did not get a result in time. Nothing was changed. Try again."
      : "WinCommander did not get a result in time. The password may already have changed. Try mounting the vault with the new password before trying again.";
  }
  if (has("broker_rejected")) return "The secure vault service refused this request. Nothing was changed. Update WinCommander and Pro together, then try again.";
  if (has("vault_not_authorized")) return "This Windows account cannot change this vault's password. Only its owner, or an account with write access to it, can.";
  if (has("caller_access_denied")) return "Windows did not let this account change the vault file or read its keyfile. Check that the file is not read-only and that you can open the keyfile.";
  if (has("vault_policy_identity_changed")) return "The selected file is not the vault WinCommander registered at this location. Check that you picked the right file.";
  if (has("vault_validation_failed")) return `Check the vault file and the passwords, then try again. Nothing was changed.${serviceDetail(detail)}`;
  if (has("vault_runtime_update_required") || has("pro_not_installed") || has("entitlement_denied") || detail.includes("feature_unknown")) {
    return "Changing a vault password needs an up-to-date WinCommander Pro. Update or install Pro from License settings, then try again.";
  }
  return "The password was not changed. Check the vault file and try again.";
}
