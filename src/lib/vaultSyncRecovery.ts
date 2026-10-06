// SPDX-License-Identifier: AGPL-3.0-or-later
export type VaultSyncRecoveryReason = "configuration_missing" | "root_missing" | "marker_missing" | "confirmation_required";
export interface VaultSyncRecoveryRoot {
  relative_path: string;
  reason: VaultSyncRecoveryReason;
  token: string;
}
export interface PersonalVaultSyncEnrollment {
  enabled: boolean;
  gui_url: string;
  recovery_required?: boolean;
  recovery_roots?: VaultSyncRecoveryRoot[];
  pairing_required?: boolean;
}
export interface VaultSyncRecoveryChoice { internalDrive: number; roots: VaultSyncRecoveryRoot[] }
export type VaultSyncRecoveryAction = "inspect" | "recreate" | "keep_paused";

const REASONS: VaultSyncRecoveryReason[] = ["configuration_missing", "root_missing", "marker_missing", "confirmation_required"];

export function vaultSyncRecoveryChoice(internalDrive: unknown, roots: unknown): VaultSyncRecoveryChoice | null {
  if (typeof internalDrive !== "number" || !Number.isInteger(internalDrive) || internalDrive < 0 || internalDrive > 25
    || !Array.isArray(roots) || roots.length === 0 || roots.length > 32) return null;
  const unique = new Set<string>();
  const parsed: VaultSyncRecoveryRoot[] = [];
  for (const root of roots) {
    if (!root || typeof root !== "object" || typeof root.relative_path !== "string"
      || root.relative_path.length === 0 || root.relative_path.length > 240
      || root.relative_path.split(/[\\/]/).some((part: string) => !part || part === "." || part === ".." || /[:\x00-\x1f]/.test(part))
      || !REASONS.includes(root.reason) || typeof root.token !== "string"
      || !/^[a-fA-F0-9]{64}$/.test(root.token) || unique.has(root.token)) return null;
    unique.add(root.token);
    parsed.push({ relative_path: root.relative_path, reason: root.reason, token: root.token });
  }
  return { internalDrive, roots: parsed };
}

export function vaultSyncRecoveryReason(reason: VaultSyncRecoveryReason): string {
  return {
    configuration_missing: "Its Syncthing configuration was removed.",
    root_missing: "The folder inside the Vault is missing.",
    marker_missing: "The folder's Syncthing safety marker is missing.",
    confirmation_required: "This folder needs your recovery decision before it can sync again.",
  }[reason];
}

export function isSyncthingSetupUrl(value: unknown): value is string {
  if (typeof value !== "string") return false;
  try {
    const url = new URL(value);
    return url.protocol === "http:" && url.hostname === "127.0.0.1" && url.username === "" && url.password === ""
      && Number(url.port || 80) > 0 && url.pathname === "/" && !url.search && !url.hash;
  } catch { return false; }
}
