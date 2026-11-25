export type VaultPasswordKind = "standard" | "decoy";
export type DecoyPasswordChoice = "decoy" | "real";

export interface VaultPasswordChangeForm {
  volumePath: string;
  vaultKind: VaultPasswordKind;
  which: DecoyPasswordChoice;
  oldPassword: string;
  newPassword: string;
  confirmPassword: string;
  oldPim: string;
  newPim: string;
  oldKeyfile: string;
  newKeyfile: string;
}

export interface VaultPasswordChangeResult {
  status: "password_changed";
  path: string;
  target: "outer" | "hidden";
}

export const EMPTY_PASSWORD_CHANGE_FORM: VaultPasswordChangeForm = {
  volumePath: "",
  vaultKind: "standard",
  which: "decoy",
  oldPassword: "",
  newPassword: "",
  confirmPassword: "",
  oldPim: "",
  newPim: "",
  oldKeyfile: "",
  newKeyfile: "",
};

const MAX_PIM = 2_147_468;
const validPim = (value: string) => !value || (/^\d+$/.test(value) && Number(value) >= 1 && Number(value) <= MAX_PIM);

/** A decoy vault keeps its decoy password in the outer header and its real one in the hidden header. */
export function passwordChangeTarget(form: Pick<VaultPasswordChangeForm, "vaultKind" | "which">): "outer" | "hidden" {
  return form.vaultKind === "decoy" && form.which === "real" ? "hidden" : "outer";
}

/** Returns the first problem to show, or null when the form can be sent. */
export function passwordChangeFormProblem(form: VaultPasswordChangeForm): string | null {
  if (!form.volumePath.trim()) return "Choose the vault file.";
  if (!form.oldPassword && !form.oldKeyfile.trim()) return "Enter the current password.";
  if (!form.newPassword && !form.newKeyfile.trim()) return "Enter a new password.";
  if (form.newPassword !== form.confirmPassword) return "The new passwords do not match.";
  if (!validPim(form.oldPim) || !validPim(form.newPim)) return "A PIM must be a whole number from 1 to 2147468, or left blank.";
  if (form.newPassword === form.oldPassword && form.newPim === form.oldPim && form.newKeyfile.trim() === form.oldKeyfile.trim()) {
    return "The new password must be different from the current one.";
  }
  return null;
}

/** Field names follow the Pro "Set-EncryptionVolumePassword" handler; lists travel as JSON text like the mount form's. */
export function buildPasswordChangeRequest(form: VaultPasswordChangeForm): Record<string, string> {
  const keyfiles = (path: string) => JSON.stringify(path.trim() ? [path.trim()] : []);
  return {
    VolumePath: form.volumePath.trim(),
    TargetKind: "file",
    Target: passwordChangeTarget(form),
    OldPassword: form.oldPassword,
    NewPassword: form.newPassword,
    OldKeyfiles: keyfiles(form.oldKeyfile),
    NewKeyfiles: keyfiles(form.newKeyfile),
    ...(form.oldPim ? { OldPim: form.oldPim } : {}),
    ...(form.newPim ? { NewPim: form.newPim } : {}),
  };
}

export const PASSWORD_CHANGE_BUSY = "Checking the current password and changing it. This can take up to a minute; keep this window open.";

export const PASSWORD_CHANGE_WARNING = "Header backups and copies of this container made before now still open with the old password.";

/** Maps the stable service and engine codes to plain wording; never echoes raw error text. */
export function vaultPasswordChangeErrorMessage(error: unknown): string {
  const detail = (error instanceof Error ? error.message : typeof error === "string" ? error : "").toLowerCase();
  const has = (code: string) => new RegExp(`(?:^|[^a-z_])(?:vault_)?${code}(?:$|[^a-z_])`).test(detail);
  if (has("wrong_header")) return "That password belongs to the other password of this decoy vault. Check which password you chose to change, or enter the current password for that one. Nothing was changed.";
  if (has("wrong_password") || has("engine_unlock_failed")) return "The current password, PIM or keyfile did not open this vault. Check them and try again. Nothing was changed.";
  if (has("new_password_collides")) return "The new password is the same as the other password of this decoy vault. Each password must be different. Nothing was changed.";
  if (has("weak_new_password")) return "The new password is too weak. Use a longer password with a mix of words, numbers and symbols. Nothing was changed.";
  if (has("volume_mounted") || has("already_mounted")) return "Dismount the vault first, then change its password. Nothing was changed.";
  if (has("engine_failed")) return "The encryption engine could not change the password. Nothing was confirmed as changed; try opening the vault with the current password before trying again.";
  if (has("vault_request_timeout") || has("vault_operation_unconfirmed") || detail.includes("pro response timeout")) {
    return "WinCommander did not get a result in time. The password may already have changed. Try mounting the vault with the new password before trying again.";
  }
  if (has("broker_rejected")) return "The secure vault service refused this request. Nothing was changed. Update WinCommander and Pro together, then try again.";
  if (has("vault_not_authorized")) return "This Windows account cannot change this vault's password. Only its owner, or an account with write access to it, can.";
  if (has("caller_access_denied")) return "Windows did not let this account change the vault file or read its keyfile. Check that the file is not read-only and that you can open the keyfile.";
  if (has("vault_policy_identity_changed")) return "The selected file is not the vault WinCommander registered at this location. Check that you picked the right file.";
  if (has("mount_state_unknown")) return "WinCommander could not confirm that this vault is dismounted. Refresh Secure Storage, dismount it if it is listed, then try again.";
  if (has("vault_validation_failed")) return "Check the vault file and the passwords, then try again. Nothing was changed.";
  if (has("vault_runtime_update_required") || has("pro_not_installed") || has("entitlement_denied") || detail.includes("feature_unknown")) {
    return "Changing a vault password needs an up-to-date WinCommander Pro. Update or install Pro from License settings, then try again.";
  }
  return "The password was not changed. Check the vault file and try again.";
}
