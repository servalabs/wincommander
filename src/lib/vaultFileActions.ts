import type { VaultDisplayName } from "./vaultRename";

/** The vault a list row offers to change. A row that does not know its file passes path null. */
export interface VaultFileTarget {
  path: string | null;
  label?: string;
  /** Lets a list that stores the path (such as a Quick Mount shortcut) follow the renamed file. */
  onRenamed?: (path: string) => void;
}

export interface VaultFileActions {
  changePassword: (target: VaultFileTarget) => void;
  rename: (target: VaultFileTarget) => void;
}

export interface VaultFileRequest {
  /** A new id remounts the dialog, so each opening starts from its row's path. */
  id: number;
  dialog: "password" | "rename";
  path: string;
  /** A row's own vault stays fixed; only the "Other vault file" entry lets the person pick a file. */
  pathLocked: boolean;
  open: boolean;
  onRenamed?: (path: string) => void;
}

export const DISMOUNT_FIRST_HINT = "Dismount this vault to change its password or rename it.";

/** The row's own path, else the path of the only named vault with the row's label, else empty. */
export function resolveVaultFilePath(target: VaultFileTarget, names: readonly VaultDisplayName[]): string {
  if (target.path?.trim()) return target.path.trim();
  const label = target.label?.trim();
  const matches = label ? names.filter(row => row.name === label) : [];
  return matches.length === 1 ? matches[0]!.path : "";
}

/** A mounted vault gets no Change Password or Rename: neither can run until it is dismounted. */
export function rowVaultFileActions(actions: VaultFileActions | null, isMounted: boolean): VaultFileActions | null {
  return isMounted ? null : actions;
}
