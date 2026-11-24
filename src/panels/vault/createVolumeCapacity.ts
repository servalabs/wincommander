import { FREE_SPACE_ROUNDING_BYTES, formatBytes, type SizeUnit } from "../../lib/stegoBackup";

const UNIT_BYTES: Record<SizeUnit, number> = { M: 1024 ** 2, G: 1024 ** 3, T: 1024 ** 4 };

export const LARGE_VAULT_BYTES = 2 * 1024 ** 3;

export type DestinationSpaceVerdict =
  | { level: "ok" }
  | { level: "warning" | "blocked"; message: string };

export function sizeToBytes(value: number, unit: SizeUnit): number {
  return Number.isFinite(value) && value > 0 ? value * UNIT_BYTES[unit] : 0;
}

export function largeVaultNotice(sizeBytes: number, isHiddenDecoy: boolean): string | null {
  if (sizeBytes < LARGE_VAULT_BYTES) return null;
  const reason = isHiddenDecoy ? " because a hidden-volume Vault fills the whole outer volume with random data" : "";
  return `A Vault this large can take several minutes to create${reason}. Keep WinCommander open until it finishes.`;
}

/**
 * `freeBytes` null means the drive could not be read; `isElevated` null means the
 * account type is unknown. Neither blocks, because only a clear shortfall may.
 */
export function assessDestinationSpace(input: {
  sizeBytes: number;
  freeBytes: number | null;
  isElevated: boolean | null;
  isDynamic: boolean;
}): DestinationSpaceVerdict {
  const { sizeBytes, freeBytes, isElevated, isDynamic } = input;
  // A dynamic container is sparse, so its nominal size is not what it takes up.
  if (freeBytes === null || sizeBytes <= 0 || isDynamic) return { level: "ok" };
  if (sizeBytes > freeBytes + FREE_SPACE_ROUNDING_BYTES) {
    return {
      level: "blocked",
      message: `This drive has about ${formatBytes(freeBytes)} free, but the Vault needs ${formatBytes(sizeBytes)}. Choose a drive with more free space or a smaller size.`,
    };
  }
  if (isElevated === false && sizeBytes * 2 > freeBytes + FREE_SPACE_ROUNDING_BYTES) {
    return {
      level: "warning",
      message: `WinCommander is not running with administrator rights, so creating this Vault may need about twice its size (${formatBytes(sizeBytes * 2)}) free on both this drive and the Windows drive while it is prepared. This drive has about ${formatBytes(freeBytes)} free.`,
    };
  }
  return { level: "ok" };
}
