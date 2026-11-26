export interface VaultDisplayName {
  path: string;
  name: string;
}

/** Explorer's "Copy as path" wraps the path in quotes; the service strips one pair too. */
export function cleanVaultPath(path: string): string {
  const trimmed = path.trim();
  return /^"[^"]*"$/.test(trimmed) ? trimmed.slice(1, -1).trim() : trimmed;
}

const SERVICE_DETAILS: Record<string, string> = {
  "rename request is invalid": "the request was not accepted",
  "password change request is invalid": "the request was not accepted",
  "the selected container could not be found": "the vault file could not be found or read",
  "personal vault path is invalid": "the vault path is not valid",
};

/** Only known service reasons are shown, so a support report can tell the cases apart without echoing raw text. */
export function serviceDetail(lowerCaseError: string): string {
  const match = Object.keys(SERVICE_DETAILS).find(reason => lowerCaseError.includes(reason));
  return match ? ` (Reason: ${SERVICE_DETAILS[match]}.)` : "";
}

/** Folds the spellings Windows and the service use for one file into one key. */
export function vaultPathKey(path: string): string {
  return cleanVaultPath(path).replace(/\//g, "\\").replace(/^\\\\\?\\|^\\\?\?\\/, "").toLowerCase();
}

export function findVaultName(names: readonly VaultDisplayName[], path: string | null | undefined): string | null {
  if (!path?.trim()) return null;
  const key = vaultPathKey(path);
  return names.find(entry => vaultPathKey(entry.path) === key)?.name ?? null;
}

/** Keeps only well-formed rows so a bad reply can never show a wrong name. */
export function readVaultDisplayNames(value: unknown): VaultDisplayName[] {
  const rows = (value as { names?: unknown } | null)?.names;
  if (!Array.isArray(rows)) throw new Error("vault_display_names_invalid");
  return rows.filter((row): row is VaultDisplayName =>
    typeof row?.path === "string" && typeof row?.name === "string" && row.path !== "" && row.name !== "");
}
