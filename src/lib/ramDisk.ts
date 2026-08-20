import type { RamDiskAutostartSettings } from "../types/settings";

/** Smallest supported RAM-disk size. Keep every creation path aligned. */
export const MIN_RAM_DISK_SIZE_MB = 256;

/**
 * Normalise saved or user-provided sizes so legacy settings cannot create an
 * unsupported RAM disk. Values are rounded because ImDisk accepts whole MB.
 */
export function normalizeRamDiskSizeMB(sizeMB: number | null | undefined): number {
  const parsed = Number(sizeMB);
  return Number.isFinite(parsed)
    ? Math.max(MIN_RAM_DISK_SIZE_MB, Math.round(parsed))
    : MIN_RAM_DISK_SIZE_MB;
}

/**
 * Turns one saved startup specification into the exact request used for both
 * immediate mounting and the next app start. A missing size is deliberately
 * rejected instead of being converted into the minimum-size disk.
 */
export function savedRamDiskMountRequest(spec: RamDiskAutostartSettings): {
  SizeMB: number;
  DriveLetter: string;
  Filesystem: "NTFS" | "FAT32" | "exFAT";
  Label: string;
  ReadOnly: boolean;
  Quick: true;
} | null {
  const configuredSizeMB = Number(spec.sizeMB);
  if (!Number.isFinite(configuredSizeMB) || configuredSizeMB < MIN_RAM_DISK_SIZE_MB) {
    return null;
  }

  return {
    SizeMB: normalizeRamDiskSizeMB(configuredSizeMB),
    DriveLetter: (spec.driveLetter || "R").toUpperCase(),
    Filesystem: spec.filesystem ?? "NTFS",
    Label: spec.label || "TEMP",
    ReadOnly: spec.readOnly ?? false,
    Quick: true,
  };
}
