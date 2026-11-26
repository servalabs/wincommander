import type { VaultPasswordKind } from "./vaultPasswordChange";

/** Same bound the service applies (bytes, like a Fleet Vault label). */
export const MAX_VAULT_NAME_BYTES = 128;

export interface VaultDisplayName {
  path: string;
  name: string;
  /** The type the vault was created as, when the service knows it. */
  kind?: VaultPasswordKind;
}

export interface VaultRenameResult {
  status: "renamed";
  name: string;
}

/** Returns the first problem to show, or null when the name can be sent. */
export function vaultNameProblem(name: string): string | null {
  const trimmed = name.trim();
  if (!trimmed) return "Enter a name.";
  if (new TextEncoder().encode(trimmed).length > MAX_VAULT_NAME_BYTES) return "The name is too long. Use a shorter one.";
  if (/\p{Cc}/u.test(trimmed)) return "The name cannot contain tabs, line breaks or other control characters.";
  return null;
}

/** Folds the spellings Windows and the service use for one file into one key. */
export function vaultPathKey(path: string): string {
  return path.trim().replace(/\//g, "\\").replace(/^\\\\\?\\|^\\\?\?\\/, "").toLowerCase();
}

function findVaultRow(names: readonly VaultDisplayName[], path: string | null | undefined): VaultDisplayName | null {
  if (!path?.trim()) return null;
  const key = vaultPathKey(path);
  return names.find(entry => vaultPathKey(entry.path) === key) ?? null;
}

export function findVaultName(names: readonly VaultDisplayName[], path: string | null | undefined): string | null {
  return findVaultRow(names, path)?.name ?? null;
}

export function findVaultKind(names: readonly VaultDisplayName[], path: string | null | undefined): VaultPasswordKind | null {
  return findVaultRow(names, path)?.kind ?? null;
}

// "dual" is the policy container_kind spelling of a decoy vault.
const readKind = (kind: unknown): VaultPasswordKind | undefined =>
  kind === "decoy" || kind === "dual" ? "decoy" : kind === "standard" ? "standard" : undefined;

/** Keeps only well-formed rows so a bad reply can never show a wrong name. */
export function readVaultDisplayNames(value: unknown): VaultDisplayName[] {
  const rows = (value as { names?: unknown } | null)?.names;
  if (!Array.isArray(rows)) throw new Error("vault_display_names_invalid");
  return rows
    .filter((row): row is VaultDisplayName & { kind?: unknown } =>
      typeof row?.path === "string" && typeof row?.name === "string" && row.path !== "" && row.name !== "")
    .map(row => {
      const kind = readKind(row.kind);
      return kind ? { path: row.path, name: row.name, kind } : { path: row.path, name: row.name };
    });
}

/** Maps the stable service codes to plain wording; never echoes raw error text. */
export function vaultRenameErrorMessage(error: unknown): string {
  const detail = (error instanceof Error ? error.message : typeof error === "string" ? error : "").toLowerCase();
  const has = (code: string) => new RegExp(`(?:^|[^a-z_])(?:vault_)?${code}(?:$|[^a-z_])`).test(detail);
  if (has("vault_not_registered")) {
    return "WinCommander has no record of this vault, so it has nowhere to keep a name. Only vaults created in WinCommander or shared through Vault Access can be renamed. Nothing was changed.";
  }
  if (has("vault_not_authorized")) return "This Windows account cannot rename this vault. Only its owner, or an account with write access to it, can.";
  if (has("volume_mounted") || has("already_mounted")) return "Dismount the vault first, then rename it. Nothing was changed.";
  if (has("mount_state_unknown")) return "WinCommander could not confirm that this vault is dismounted. Refresh Secure Storage, dismount it if it is listed, then try again.";
  if (has("vault_policy_identity_changed")) return "The selected file is not the vault WinCommander registered at this location. Check that you picked the right file.";
  if (has("vault_rename_not_saved") || has("vault_policy_unavailable")) return "The new name could not be saved. The vault keeps its old name.";
  if (has("vault_session_unavailable")) return "Renaming needs a signed-in Windows desktop session. Sign in to Windows, then try again.";
  if (has("vault_request_timeout") || has("vault_operation_unconfirmed") || detail.includes("did not confirm")) {
    return "WinCommander did not get a result in time. Refresh Secure Storage to see whether the new name was saved.";
  }
  if (has("vault_validation_failed")) return "Check the vault file and the name, then try again. Nothing was changed.";
  return "The vault was not renamed. Check the vault file and try again.";
}
