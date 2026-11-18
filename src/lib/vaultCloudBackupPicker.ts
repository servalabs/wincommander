// SPDX-License-Identifier: AGPL-3.0-or-later
import { validCloudBackupPath } from "./vaultCloudBackup";

/** Convert a native picker result to a path under the currently mounted drive. */
export function pickedVaultPath(driveLabel: string, selected: string, allowRoot = false): string | null {
  const drive = /^([a-z]):[\\/]?$/i.exec(driveLabel.trim());
  if (!drive) return null;
  const normalized = selected.replaceAll("/", "\\");
  const prefix = `${drive[1]}:\\`;
  if (normalized.slice(0, prefix.length).toLowerCase() !== prefix.toLowerCase()) return null;
  const relative = normalized.slice(prefix.length).replace(/\\+$/, "");
  if (!relative) return allowRoot ? "" : null;
  return validCloudBackupPath(relative) ? relative : null;
}

export function addPickedVaultSources(drive: string, current: string, chosen: string[]): string | null {
  const selected = chosen.map(path => pickedVaultPath(drive, path));
  if (selected.some(path => path === null)) return null;
  const paths = current.split(/\r?\n/).map(value => value.trim()).filter(Boolean);
  for (const path of selected as string[]) {
    if (!paths.some(existing => existing.replaceAll("/", "\\").toLowerCase() === path.toLowerCase())) paths.push(path);
  }
  return paths.join("\n");
}

export function restorePathInPickedFolder(drive: string, selected: string, current: string): string | null {
  const parent = pickedVaultPath(drive, selected, true);
  if (parent === null) return null;
  const basename = current.split(/[\\/]/).at(-1) || "Restored files";
  const name = validCloudBackupPath(basename) ? basename : "Restored files";
  return parent ? `${parent}\\${name}` : name;
}
