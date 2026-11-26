export type VaultPasswordKind = "standard" | "decoy";

/** One header's credentials. A decoy vault keeps its decoy password in the outer header and its real one in the hidden header. */
export interface HeaderCredentials {
  oldPassword: string;
  newPassword: string;
  confirmPassword: string;
  oldPim: string;
  newPim: string;
  oldKeyfile: string;
  newKeyfile: string;
}

export interface VaultPasswordChangeForm {
  volumePath: string;
  vaultKind: VaultPasswordKind;
  /** The only password of a standard vault, or the decoy password. */
  outer: HeaderCredentials;
  /** The real password of a decoy vault; unused for a standard vault. */
  hidden: HeaderCredentials;
}

export interface VaultPasswordChangeResult {
  status: "password_changed";
  path: string;
  target: "outer" | "hidden" | "both";
}

export const EMPTY_HEADER_CREDENTIALS: HeaderCredentials = {
  oldPassword: "",
  newPassword: "",
  confirmPassword: "",
  oldPim: "",
  newPim: "",
  oldKeyfile: "",
  newKeyfile: "",
};

export const EMPTY_PASSWORD_CHANGE_FORM: VaultPasswordChangeForm = {
  volumePath: "",
  vaultKind: "standard",
  outer: EMPTY_HEADER_CREDENTIALS,
  hidden: EMPTY_HEADER_CREDENTIALS,
};

const MAX_PIM = 2_147_468;
const PIM_PROBLEM = "A PIM must be a whole number from 1 to 2147468, or left blank.";
const validPim = (value: string) => !value || (/^\d+$/.test(value) && Number(value) >= 1 && Number(value) <= MAX_PIM);
const hasCurrent = (header: HeaderCredentials) => Boolean(header.oldPassword || header.oldKeyfile.trim());
const hasNew = (header: HeaderCredentials) => Boolean(header.newPassword || header.newKeyfile.trim());
const pimsValid = (header: HeaderCredentials) => validPim(header.oldPim) && validPim(header.newPim);
const unchanged = (header: HeaderCredentials) => header.newPassword === header.oldPassword
  && header.newPim === header.oldPim && header.newKeyfile.trim() === header.oldKeyfile.trim();

function decoyFormProblem({ outer: decoy, hidden: real }: VaultPasswordChangeForm): string | null {
  if (!hasCurrent(decoy)) return "Enter the current decoy password.";
  if (!hasCurrent(real)) return "Enter the current real password.";
  if (!hasNew(decoy)) return "Enter a new decoy password.";
  if (!hasNew(real)) return "Enter a new real password.";
  if (decoy.newPassword !== decoy.confirmPassword) return "The new decoy passwords do not match.";
  if (real.newPassword !== real.confirmPassword) return "The new real passwords do not match.";
  if (!pimsValid(decoy) || !pimsValid(real)) return PIM_PROBLEM;
  if (decoy.newPassword === real.newPassword && decoy.newKeyfile.trim() === real.newKeyfile.trim()) {
    return "The new decoy and real passwords must be different.";
  }
  if (unchanged(decoy) && unchanged(real)) return "Change at least one of the two passwords.";
  return null;
}

/** Returns the first problem to show, or null when the form can be sent. */
export function passwordChangeFormProblem(form: VaultPasswordChangeForm): string | null {
  if (!form.volumePath.trim()) return "Choose the vault file.";
  if (form.vaultKind === "decoy") return decoyFormProblem(form);
  const header = form.outer;
  if (!hasCurrent(header)) return "Enter the current password.";
  if (!hasNew(header)) return "Enter a new password.";
  if (header.newPassword !== header.confirmPassword) return "The new passwords do not match.";
  if (!pimsValid(header)) return PIM_PROBLEM;
  if (unchanged(header)) return "The new password must be different from the current one.";
  return null;
}

/** The form with every typed password removed, so none outlives the request. */
export function withoutPasswords(form: VaultPasswordChangeForm): VaultPasswordChangeForm {
  const clear = (header: HeaderCredentials) => ({ ...header, oldPassword: "", newPassword: "", confirmPassword: "" });
  return { ...form, outer: clear(form.outer), hidden: clear(form.hidden) };
}

const keyfileList = (path: string) => JSON.stringify(path.trim() ? [path.trim()] : []);

function headerFields(header: HeaderCredentials, prefix: "" | "Hidden"): Record<string, string> {
  return {
    [`Old${prefix}Password`]: header.oldPassword,
    [`New${prefix}Password`]: header.newPassword,
    [`Old${prefix}Keyfiles`]: keyfileList(header.oldKeyfile),
    [`New${prefix}Keyfiles`]: keyfileList(header.newKeyfile),
    ...(header.oldPim ? { [`Old${prefix}Pim`]: header.oldPim } : {}),
    ...(header.newPim ? { [`New${prefix}Pim`]: header.newPim } : {}),
  };
}

/** Field names follow the Pro "Set-EncryptionVolumePassword" handler; lists travel as JSON text like the mount form's. */
export function buildPasswordChangeRequest(form: VaultPasswordChangeForm): Record<string, string> {
  const decoy = form.vaultKind === "decoy";
  return {
    VolumePath: form.volumePath.trim(),
    TargetKind: "file",
    // "both" makes the service check both current passwords before it writes either header.
    Target: decoy ? "both" : "outer",
    ...headerFields(form.outer, ""),
    ...(decoy ? headerFields(form.hidden, "Hidden") : {}),
  };
}

export const PASSWORD_CHANGE_BUSY = "Checking the current password and changing it. This can take up to a minute; keep this window open.";

export const PASSWORD_CHANGE_WARNING = "Header backups and copies of this container made before now still open with the old password.";

export const VAULT_KIND_HINT = "A decoy vault has two passwords. WinCommander can't tell from the file whether a vault has a hidden part, so choose the type you created.";

/** Maps the stable service and engine codes to plain wording; never echoes raw error text. */
export function vaultPasswordChangeErrorMessage(error: unknown): string {
  const detail = (error instanceof Error ? error.message : typeof error === "string" ? error : "").toLowerCase();
  const has = (code: string) => new RegExp(`(?:^|[^a-z_])(?:vault_)?${code}(?:$|[^a-z_])`).test(detail);
  if (has("wrong_both_passwords")) return "Both current passwords are wrong. Nothing was changed.";
  if (has("wrong_decoy_password")) return "The current decoy password is wrong. Nothing was changed.";
  if (has("wrong_real_password")) return "The current real password is wrong, or this vault has only one password. Nothing was changed.";
  if (has("wrong_header")) return "This password is the real password of a decoy vault. Choose \"Decoy vault\" and enter both passwords. Nothing was changed.";
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
