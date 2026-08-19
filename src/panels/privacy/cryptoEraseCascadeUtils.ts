// src/panels/privacy/cryptoEraseCascadeUtils.ts
//
// Pure helpers for CryptoEraseTargetsSection.tsx — the target pickers for
// the `bitlocker_erase` / `veracrypt_header_destroy` destruct steps. Kept
// separate + tested for the same reason as removeUsersUtils.ts: settings
// patches deep-merge objects but REPLACE arrays wholesale, so every
// selection change must send the FULL desired array, never a delta.

import type { VeraCryptDeviceEraseTarget } from "../../types/settings";

const normDrive = (s: string): string => s.trim().replace(/\\+$/, "").toUpperCase();

/** Returns the full next array with `drive` (normalized to "X:") toggled
 *  in/out of the BitLocker cascade-target list. */
export function toggleBitlockerDrive(current: string[], drive: string): string[] {
  const norm = normDrive(drive);
  const normalized = current.map(normDrive);
  return normalized.includes(norm)
    ? current.filter((d) => normDrive(d) !== norm)
    : [...current, norm];
}

export function isBitlockerDriveSelected(current: string[], drive: string): boolean {
  const norm = normDrive(drive);
  return current.some((d) => normDrive(d) === norm);
}

/** Adds a VeraCrypt container path if not already present (case-sensitive —
 *  unlike drive letters, paths are case-sensitive on some filesystems and we
 *  must not silently collapse two distinct paths that only differ in case). */
export function addVeracryptPath(current: string[], path: string): string[] {
  const trimmed = path.trim();
  if (!trimmed || current.includes(trimmed)) return current;
  return [...current, trimmed];
}

export function removeVeracryptPath(current: string[], path: string): string[] {
  return current.filter((p) => p !== path);
}

const normalizeIdentityPart = (value: string): string =>
  value.trim().replace(/^\{(.+)\}$/, "$1").toLowerCase();

/** Stable identity used for an explicitly enrolled raw partition. Device paths
 * can be reassigned after storage topology changes, so they are only a fallback
 * for old/incomplete settings. The backend re-probes every identity field again
 * immediately before destruction. */
export function veracryptDeviceIdentity(target: VeraCryptDeviceEraseTarget): string {
  const diskId = normalizeIdentityPart(target.diskUniqueId);
  const partitionGuid = normalizeIdentityPart(target.partitionGuid);
  if (diskId && partitionGuid && target.offsetBytes > 0 && target.sizeBytes > 0) {
    return `${diskId}|${partitionGuid}|${target.offsetBytes}|${target.sizeBytes}`;
  }
  return normalizeIdentityPart(target.devicePath);
}

export function removeVeracryptDevice(
  current: VeraCryptDeviceEraseTarget[],
  target: VeraCryptDeviceEraseTarget,
): VeraCryptDeviceEraseTarget[] {
  const identity = veracryptDeviceIdentity(target);
  return current.filter((candidate) => veracryptDeviceIdentity(candidate) !== identity);
}
