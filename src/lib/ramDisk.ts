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
