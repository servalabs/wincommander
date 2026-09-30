import { parseProReleaseVersion } from "./proReleaseCompatibility";

const KEY = "wincommander.pendingProUpdate";
const MAX_AGE_MS = 7 * 24 * 60 * 60 * 1000;
type UpdateStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

export interface PendingProUpdate {
  freeVersion: string;
  requestedAt: number;
}

export function rememberPendingProUpdate(storage: UpdateStorage, freeVersion: string, now = Date.now()): void {
  if (!parseProReleaseVersion(freeVersion)) throw new Error("The update version could not be verified.");
  storage.setItem(KEY, JSON.stringify({ freeVersion, requestedAt: now }));
}

export function readPendingProUpdate(storage: UpdateStorage, now = Date.now()): PendingProUpdate | null {
  try {
    const value: unknown = JSON.parse(storage.getItem(KEY) ?? "null");
    if (value && typeof value === "object" && "freeVersion" in value && "requestedAt" in value
      && typeof value.freeVersion === "string" && parseProReleaseVersion(value.freeVersion)
      && typeof value.requestedAt === "number" && Number.isFinite(value.requestedAt)
      && value.requestedAt <= now && now - value.requestedAt <= MAX_AGE_MS) {
      return { freeVersion: value.freeVersion, requestedAt: value.requestedAt };
    }
    storage.removeItem(KEY);
  } catch { /* Unavailable storage must not initiate an update. */ }
  return null;
}

export function clearPendingProUpdate(storage: UpdateStorage, expectedVersion: string): void {
  if (readPendingProUpdate(storage)?.freeVersion === expectedVersion) storage.removeItem(KEY);
}

export function canResumeProUpdate(pending: PendingProUpdate | null, currentVersion: string): boolean {
  return pending !== null && pending.freeVersion === currentVersion;
}

export async function handoffFreeUpdate(
  storage: UpdateStorage,
  version: string | null,
  includePro: boolean,
  probeInstalledPro: () => Promise<boolean>,
  installFree: () => Promise<unknown>,
): Promise<void> {
  let pending = false;
  if (includePro) {
    try {
      if (!version) throw new Error("The target WinCommander version is unavailable.");
      if (await probeInstalledPro()) {
        rememberPendingProUpdate(storage, version);
        pending = true;
      }
    } catch (error) {
      throw new Error(`The combined update could not be prepared. ${error instanceof Error ? error.message : String(error)} Nothing was installed; retry the update.`);
    }
  }
  try {
    await installFree();
  } catch (error) {
    if (pending && version) clearPendingProUpdate(storage, version);
    throw error;
  }
}
