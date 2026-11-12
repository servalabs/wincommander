import type { PersonalVaultMountedVolume, VaultAuthorizedEntry } from "@/panels/fleet/vaultAccessTypes";

export type SecureStorageDismountTarget =
  | { kind: "personal"; internalDrive: number }
  | { kind: "fleet"; entryId: string }
  | null;

interface EngineVolumeRow {
  letter: string;
  internalDrive?: number;
}

const normalizeDriveLetter = (letter: string | null | undefined) =>
  letter?.trim().replace(/[\\/:]/g, "").toLocaleUpperCase() ?? "";
const isValidInternalDrive = (drive: number | undefined): drive is number =>
  typeof drive === "number" && Number.isInteger(drive) && drive >= 0 && drive <= 25;

/**
 * Engine enumeration is machine-wide and may include a Fleet mount belonging
 * to another account. A row is renderable only when the service's
 * caller-filtered personal or Fleet ledger uniquely identifies it.
 */
export function secureStorageMountIsAuthorized(
  volume: EngineVolumeRow,
  personalMounts: PersonalVaultMountedVolume[],
  fleetEntries: VaultAuthorizedEntry[],
): boolean {
  const letter = normalizeDriveLetter(volume.letter);
  const internalDrive = volume.internalDrive;
  if (!letter || !isValidInternalDrive(internalDrive)) return false;

  const fleetMatches = fleetEntries.filter(
    (entry) => entry.mount_state === "mounted"
      && normalizeDriveLetter(entry.drive_letter) === letter
      && entry.internal_drive === internalDrive,
  );
  const personalMatches = personalMounts.filter(
    (mount) => normalizeDriveLetter(mount.drive_letter) === letter
      && Number.isInteger(mount.internal_drive)
      && mount.internal_drive >= 0
      && mount.internal_drive <= 25,
  );

  // A collision across ledgers or duplicate service records is ambiguous;
  // never reveal a path based on the engine's machine-wide volume list alone.
  if (fleetMatches.length + personalMatches.length !== 1) return false;
  if (fleetMatches.length === 1) return true;

  const personal = personalMatches[0];
  return internalDrive === personal.internal_drive;
}

/**
 * Resolve a displayed engine row only when the service ledger identifies its
 * owner and exact dismount route. Unknown/ambiguous rows deliberately have no
 * action; renderer-observed drive letters are never treated as authority.
 */
export function secureStorageDismountTarget(
  volume: EngineVolumeRow,
  personalMounts: PersonalVaultMountedVolume[],
  fleetEntries: VaultAuthorizedEntry[],
): SecureStorageDismountTarget {
  const letter = normalizeDriveLetter(volume.letter);
  const internalDrive = volume.internalDrive;
  if (!letter || !isValidInternalDrive(internalDrive)) return null;

  const fleetMatches = fleetEntries.filter(
    (entry) => entry.mount_state === "mounted"
      && normalizeDriveLetter(entry.drive_letter) === letter
      && entry.internal_drive === internalDrive,
  );
  // A Fleet mount always stays on the entry-id route, even if the service says
  // this caller may not close it. Never reinterpret it as an ordinary mount.
  if (fleetMatches.length > 0) {
    if (fleetMatches.length !== 1 || !fleetMatches[0].can_dismount || !fleetMatches[0].entry_id) return null;
    return { kind: "fleet", entryId: fleetMatches[0].entry_id };
  }

  const personalMatches = personalMounts.filter(
    (mount) => normalizeDriveLetter(mount.drive_letter) === letter
      && Number.isInteger(mount.internal_drive)
      && mount.internal_drive >= 0
      && mount.internal_drive <= 25,
  );
  if (personalMatches.length !== 1) return null;
  const personal = personalMatches[0];
  if (internalDrive !== personal.internal_drive) return null;
  return { kind: "personal", internalDrive: personal.internal_drive };
}
