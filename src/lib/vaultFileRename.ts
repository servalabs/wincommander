import { cleanVaultPath, serviceDetail } from "./vaultRename";
import {
  EMPTY_HEADER_CREDENTIALS,
  REAL_PASSWORD_REQUIRED,
  WRONG_PASSWORD,
  currentPasswordProblem,
  type HeaderCredentials,
} from "./vaultPasswordChange";

/** Arguments of the `vault_rename` Tauri command; the password and keyfiles live only in this object. */
export interface VaultRenameRequest {
  containerPath: string;
  newName: string;
  password: string;
  pim: number | null;
  keyfiles: string[];
}

export interface VaultRenameResult {
  path: string;
  name: string;
}

export const RENAME_BUSY = "Checking the password and renaming the file…";

export const RENAME_PASSWORD_NOTE = "Checking the password can take up to about 30 seconds.";

export const POLICY_RENAME_REFUSED = "Only this vault's owner or an administrator can rename it.";

const MAX_FILE_NAME_UNITS = 255;
const RESERVED_DEVICE_NAME = /^(con|prn|aux|nul|com[1-9¹²³]|lpt[1-9¹²³])(\.|$)/i;

export function vaultFileName(path: string): string {
  return cleanVaultPath(path).split(/[\\/]/).pop() ?? "";
}

const extensionOf = (name: string) => {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot) : "";
};

/** The service keeps the old extension when the new name has none. */
export function renamedFileName(newName: string, currentName: string): string {
  return extensionOf(newName) ? newName : newName + extensionOf(currentName);
}

/** Mirrors the service's name rules so most mistakes show before the password check runs. */
export function newFileNameProblem(newName: string, currentPath: string): string | null {
  if (!newName) return "Enter the new file name.";
  if (/[\\/:*?"<>|\p{Cc}]/u.test(newName)) return 'A file name cannot contain \\ / : * ? " < > | or control characters.';
  if (/[. ]$/.test(newName)) return "A file name cannot end with a dot or a space.";
  if (RESERVED_DEVICE_NAME.test(newName)) return "Windows reserves that name. Choose another one.";
  const currentName = vaultFileName(currentPath);
  const target = renamedFileName(newName, currentName);
  if (target.length > MAX_FILE_NAME_UNITS) return "The name is too long. Use 255 characters or fewer.";
  if (target.toLowerCase() === currentName.toLowerCase()) return "This is already the file's name. Enter a different one.";
  return null;
}

export function buildRenameRequest(volumePath: string, newName: string, credentials: HeaderCredentials): VaultRenameRequest {
  const keyfile = credentials.oldKeyfile.trim();
  return {
    containerPath: cleanVaultPath(volumePath),
    newName,
    password: credentials.oldPassword,
    pim: credentials.oldPim ? Number(credentials.oldPim) : null,
    keyfiles: keyfile ? [keyfile] : [],
  };
}

/** Accepts only the contract's success shape, so a malformed reply is never shown as a rename. */
export function readRenameResult(data: unknown): VaultRenameResult | null {
  const reply = data as { status?: unknown; path?: unknown; name?: unknown } | null;
  return reply?.status === "renamed" && typeof reply.path === "string" && reply.path && typeof reply.name === "string" && reply.name
    ? { path: reply.path, name: reply.name }
    : null;
}

/** Maps the stable service codes to plain wording; never echoes raw error text. */
export function vaultRenameErrorMessage(error: unknown): string {
  const detail = (error instanceof Error ? error.message : typeof error === "string" ? error : "").toLowerCase();
  const has = (code: string) => new RegExp(`(?:^|[^a-z_])(?:vault_)?${code}(?:$|[^a-z_])`).test(detail);
  if (has("vault_policy_managed")) return POLICY_RENAME_REFUSED;
  if (has("real_password_required")) return REAL_PASSWORD_REQUIRED;
  if (has("wrong_password") || has("engine_unlock_failed")) return WRONG_PASSWORD;
  if (has("name_invalid")) return 'Windows does not allow that file name. Avoid \\ / : * ? " < > |, a dot or space at the end, and names such as CON or NUL. Nothing was changed.';
  if (has("name_exists")) return "A file with that name already exists in this folder. Choose another name. Nothing was changed.";
  if (has("name_unchanged")) return "This is already the file's name. Enter a different one.";
  if (has("volume_mounted") || has("already_mounted")) return "Dismount the vault first, then rename it. Nothing was changed.";
  if (has("mount_state_unknown")) return "WinCommander could not confirm that this vault is dismounted. Refresh Secure Storage, dismount it if it is listed, then try again.";
  if (has("vault_rename_not_saved")) return "WinCommander could not update its records for the new name, so the file kept its old name. Nothing was changed. Try again.";
  if (has("vault_file_not_found")) return "WinCommander could not find the vault file. Check the path, then try again. Nothing was changed.";
  if (has("vault_file_in_use")) return "Another program is using the vault file. Close it, then try again. Nothing was changed.";
  if (has("vault_access_denied") || has("caller_access_denied")) {
    return "Windows did not let this account rename the vault file. The account needs permission to change the file and its folder. Nothing was changed.";
  }
  if (has("vault_not_authorized")) return "This Windows account cannot rename this vault. Only its owner, or an account with write access to it, can.";
  if (has("vault_policy_identity_changed")) return "The selected file is not the vault WinCommander registered at this location. Check that you picked the right file.";
  if (has("vault_policy_unavailable")) return "WinCommander could not check this vault's permissions. Nothing was changed. Try again.";
  if (has("engine_failed")) return "The encryption engine could not check the password. Nothing was changed. Try again.";
  if (has("vault_session_unavailable")) return "Renaming needs a signed-in Windows desktop session. Sign in to Windows, then try again.";
  if (has("vault_request_timeout") || has("vault_operation_unconfirmed") || detail.includes("did not confirm")) {
    return "WinCommander did not get a result in time. Refresh Secure Storage to see whether the file was renamed.";
  }
  if (has("broker_rejected")) return "The secure vault service refused this request. Nothing was changed. Update WinCommander and Pro together, then try again.";
  if (has("vault_runtime_update_required") || has("pro_not_installed") || has("entitlement_denied") || detail.includes("feature_unknown")) {
    return "Renaming a vault needs an up-to-date WinCommander Pro. Update or install Pro from License settings, then try again.";
  }
  if (has("vault_validation_failed")) return `Check the vault file and the name, then try again. Nothing was changed.${serviceDetail(detail)}`;
  return "The vault was not renamed. Check the vault file and try again.";
}

export interface RenameFlow {
  volumePath: string;
  newName: string;
  /** Only the current password, PIM and keyfile are used. */
  credentials: HeaderCredentials;
  busy: boolean;
  /** The new file name once the service confirmed the rename. */
  renamed: string;
  failure: string;
}

export type RenameEvent =
  | { type: "path"; volumePath: string }
  | { type: "name"; newName: string }
  | { type: "edit"; field: "oldPassword" | "oldPim" | "oldKeyfile"; value: string }
  | { type: "send" }
  | { type: "renamed"; result: VaultRenameResult }
  | { type: "failed"; error: unknown }
  | { type: "notice"; failure: string }
  | { type: "reset" };

export function startRename(volumePath = ""): RenameFlow {
  return { volumePath, newName: vaultFileName(volumePath), credentials: EMPTY_HEADER_CREDENTIALS, busy: false, renamed: "", failure: "" };
}

export function renameProblem(flow: RenameFlow): string | null {
  if (!flow.volumePath.trim()) return "Choose the vault file.";
  return newFileNameProblem(flow.newName, flow.volumePath) ?? currentPasswordProblem(flow.volumePath, flow.credentials);
}

export function renameReducer(flow: RenameFlow, event: RenameEvent): RenameFlow {
  if (flow.renamed && event.type !== "reset") return flow;
  switch (event.type) {
    case "path": {
      // A name the person has not edited follows the newly chosen file.
      const untouched = flow.newName === vaultFileName(flow.volumePath);
      return { ...flow, volumePath: event.volumePath, newName: untouched ? vaultFileName(event.volumePath) : flow.newName, failure: "" };
    }
    case "name":
      return { ...flow, newName: event.newName, failure: "" };
    case "edit":
      return { ...flow, credentials: { ...flow.credentials, [event.field]: event.value }, failure: "" };
    case "send":
      // The request is built before this event; the password never stays in state once sent.
      return { ...flow, busy: true, failure: "", credentials: { ...flow.credentials, oldPassword: "" } };
    case "renamed":
      return { ...flow, busy: false, volumePath: event.result.path, newName: event.result.name, credentials: EMPTY_HEADER_CREDENTIALS, renamed: event.result.name, failure: "" };
    case "failed":
      return { ...flow, busy: false, failure: vaultRenameErrorMessage(event.error) };
    case "notice":
      return { ...flow, failure: event.failure };
    case "reset":
      return startRename();
  }
}
