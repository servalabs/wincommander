import type { VaultAccessEntry } from "./vaultAccessTypes";
import type { FleetAccessDirectory, FleetAccessUser } from "./accessControlTypes";

const LEGACY_VAULT_STORAGE_KEY = "wincommander.fleet.vault-policy.v1";

export interface LegacyVaultDraftImport {
  entries: VaultAccessEntry[];
  droppedPrincipalCount: number;
  skippedVolumeCount: number;
}

function normalized(value: string) {
  return value.trim().toLocaleLowerCase();
}

function availableUsers(directory: FleetAccessDirectory) {
  return directory.users.filter(user => user.isAvailable !== false);
}

function uniqueUserPrincipal(value: string, users: FleetAccessUser[]): string | null {
  const key = normalized(value);
  const matches = users.filter(user =>
    normalized(user.id) === key
    || normalized(user.username) === key
    || (user.sid !== undefined && normalized(user.sid) === key));
  return matches.length === 1 ? matches[0]!.username : null;
}

/** Resolve a retired planner's local IDs against the current Windows-account
 * directory. Arbitrary planner strings are never carried into the service
 * policy: an unambiguous user becomes its current name; an unambiguous access
 * group becomes its configured Windows group name. */
function resolveLegacyPrincipal(value: string, directory: FleetAccessDirectory): string | null {
  const user = uniqueUserPrincipal(value, availableUsers(directory));
  if (user) return user;
  const key = normalized(value);
  const matches = directory.groups.filter(group =>
    normalized(group.id) === key || normalized(group.name) === key || normalized(group.localGroup) === key);
  return matches.length === 1 ? matches[0]!.localGroup : null;
}

function resolveLegacyOwner(value: string | undefined, directory: FleetAccessDirectory): string | null {
  if (value?.trim()) return uniqueUserPrincipal(value, availableUsers(directory));
  const current = availableUsers(directory).filter(user => user.isCurrent);
  return current.length === 1 ? current[0]!.username : null;
}

/** Convert an already-parsed retired planner into a review-only draft. */
export function importLegacyVaultDraft(
  parsed: unknown,
  directory: FleetAccessDirectory,
): LegacyVaultDraftImport | null {
  if (!parsed || typeof parsed !== "object") return null;
  const candidate = parsed as { ownerPrincipal?: unknown; volumes?: unknown };
  if (!Array.isArray(candidate.volumes)) return null;
  const legacyOwner = typeof candidate.ownerPrincipal === "string" ? candidate.ownerPrincipal : undefined;
  let droppedPrincipalCount = 0;
  let skippedVolumeCount = 0;
  const entries = candidate.volumes.flatMap((volume, index) => {
    if (!volume || typeof volume !== "object") {
      skippedVolumeCount += 1;
      return [];
    }
    const legacy = volume as { id?: unknown; label?: unknown; driveLetter?: unknown; groupPermissions?: unknown; ownerOnly?: unknown };
    const owner = resolveLegacyOwner(legacyOwner, directory);
    if (!owner) {
      skippedVolumeCount += 1;
      return [];
    }
    const grants = legacy.groupPermissions && typeof legacy.groupPermissions === "object"
      ? Object.entries(legacy.groupPermissions).flatMap(([legacyPrincipal, access]) => {
        if (access !== "read" && access !== "write") return [];
        const principal_name = resolveLegacyPrincipal(legacyPrincipal, directory);
        if (!principal_name) {
          droppedPrincipalCount += 1;
          return [];
        }
        return [{ principal_name, access }];
      })
      : [];
    return [{
      id: typeof legacy.id === "string" && legacy.id ? legacy.id : `legacy-${index + 1}`,
      label: typeof legacy.label === "string" && legacy.label ? legacy.label : `Imported vault ${index + 1}`,
      // Old raw/partition backing paths are intentionally not carried into
      // the file-container policy. An admin must choose a valid container.
      container_path: "",
      container_kind: "standard",
      owner_account: owner,
      grants: grants.length > 0 ? grants : [{ principal_name: owner, access: "write" }],
      mount: {
        presentation: legacy.ownerOnly === true ? "per-user" : "machine",
        preferred_letter: typeof legacy.driveLetter === "string" && /^[A-Z]$/i.test(legacy.driveLetter)
          ? legacy.driveLetter.toUpperCase() : undefined,
      },
    } satisfies VaultAccessEntry];
  });
  return { entries, droppedPrincipalCount, skippedVolumeCount };
}

/**
 * Reads the retired browser planner only after an administrator explicitly
 * asks to inspect it. The result is a draft, never active service policy.
 */
export function readUntrustedLegacyVaultDraft(directory: FleetAccessDirectory): LegacyVaultDraftImport | null {
  try {
    const raw = localStorage.getItem(LEGACY_VAULT_STORAGE_KEY);
    if (!raw) return null;
    return importLegacyVaultDraft(JSON.parse(raw), directory);
  } catch {
    return null;
  }
}
