import type { VaultPasswordKind } from "./vaultPasswordChange";
import type { VaultDisplayName } from "./vaultRename";

/** The vault a list row offers to change. A row that does not know its file passes path null. */
export interface VaultFileTarget {
  path: string | null;
  label?: string;
  kind?: VaultPasswordKind;
}

export interface VaultFileActions {
  /** Absent without Pro: the password change runs in the Pro engine. */
  changePassword?: (target: VaultFileTarget) => void;
  rename: (target: VaultFileTarget) => void;
}

export interface VaultFileRequest {
  /** A new id remounts the dialog, so each opening starts from its row's path. */
  id: number;
  dialog: "password" | "rename";
  path: string;
  kind?: VaultPasswordKind;
  open: boolean;
}

export const DISMOUNT_FIRST_HINT = "Dismount this vault to change its password or rename it.";

export function vaultKindFromContainer(containerKind: string): VaultPasswordKind {
  return containerKind === "dual" ? "decoy" : "standard";
}

/** The row's own path, else the path of the only named vault with the row's label, else empty. */
export function resolveVaultFilePath(target: VaultFileTarget, names: readonly VaultDisplayName[]): string {
  if (target.path?.trim()) return target.path.trim();
  const label = target.label?.trim();
  const matches = label ? names.filter(row => row.name === label) : [];
  return matches.length === 1 ? matches[0]!.path : "";
}
