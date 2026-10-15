import type { EncryptionStatus } from "@/hooks/useBackend";

export interface QuickMountExplorerTarget {
  drive: string;
  containerPath: string;
  internalDrive: number;
  receipt: string;
}

const normalizePath = (value: string) => value.replace(/^\\\\\?\\/, "").replace(/\//g, "\\").toLowerCase();
const normalizeDrive = (value: string) => value.replace(/:$/, "").toUpperCase();

export function quickMountExplorerTarget(
  status: EncryptionStatus | null, statusError: string | null,
  drive: string | null, containerPath: string | undefined, internalDrive?: number,
): QuickMountExplorerTarget | null {
  if (statusError || !drive || !containerPath || !/^[D-Z]$/.test(normalizeDrive(drive))) return null;
  const matches = status?.volumes?.filter(volume => volume.accessible === true && volume.browseAllowed === true
    && volume.cleanupRequired !== true
    && normalizeDrive(volume.letter) === normalizeDrive(drive)
    && !!volume.path && normalizePath(volume.path) === normalizePath(containerPath)
    && Number.isInteger(volume.internalDrive) && volume.internalDrive! >= 0 && volume.internalDrive! <= 25
    && (internalDrive === undefined || volume.internalDrive === internalDrive)
    && typeof volume.quickMountReceipt === "string" && /^qm1:[a-f0-9]{64}$/.test(volume.quickMountReceipt));
  if (matches?.length !== 1) return null;
  const volume = matches[0];
  return { drive: `${normalizeDrive(drive)}:`, containerPath, internalDrive: volume.internalDrive!, receipt: volume.quickMountReceipt! };
}

export function canOpenQuickMountDrive(
  status: EncryptionStatus | null, statusError: string | null,
  drive: string | null, containerPath: string | undefined,
  expected?: QuickMountExplorerTarget,
): boolean {
  const target = quickMountExplorerTarget(status, statusError, drive, containerPath, expected?.internalDrive);
  return target !== null && (!expected || target.receipt === expected.receipt);
}

export function quickMountExplorerTargetForDrive(status: EncryptionStatus | null, drive: string): QuickMountExplorerTarget | null {
  const matches = status?.volumes?.filter(volume => normalizeDrive(volume.letter) === normalizeDrive(drive));
  if (matches?.length !== 1 || !matches[0].path) return null;
  return quickMountExplorerTarget(status, null, drive, matches[0].path, matches[0].internalDrive);
}

export async function openConfirmedQuickMountTarget(
  target: QuickMountExplorerTarget,
  readStatus: () => Promise<EncryptionStatus | null>,
  verify: (drive: string) => Promise<{ drive: string; accessible: boolean }>,
  open: (drive: string, expectedMountReceipt: string) => Promise<void>,
): Promise<void> {
  const before = await readStatus();
  if (!canOpenQuickMountDrive(before, null, target.drive, target.containerPath, target)) throw new Error("vault_quick_mount_open_unconfirmed");
  const ready = await verify(target.drive);
  if (!ready.accessible || normalizeDrive(ready.drive) !== normalizeDrive(target.drive)) throw new Error("vault_quick_mount_open_unconfirmed");
  const after = await readStatus();
  if (!canOpenQuickMountDrive(after, null, target.drive, target.containerPath, target)) throw new Error("vault_quick_mount_open_unconfirmed");
  await open(target.drive, target.receipt);
}

export async function openQuickMountIfEnabled(
  enabled: boolean, ...args: Parameters<typeof openConfirmedQuickMountTarget>
): Promise<void> {
  if (enabled) await openConfirmedQuickMountTarget(...args);
}
