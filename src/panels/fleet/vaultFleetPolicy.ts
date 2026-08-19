import type { FleetAccessDirectory, FleetAccessGroup } from "./accessControlTypes";
import { resolveGroupPolicy } from "./groupPolicyResolution";
import type { VaultFleetPolicy, VaultMatrixRow, VaultVolumePolicy } from "./vaultFleetTypes";

type LegacyVaultVolume = Omit<VaultVolumePolicy, "groupPermissions"> & {
  groupPermissions?: VaultVolumePolicy["groupPermissions"];
  allowedGroups?: string[];
  groupReadOnly?: boolean;
};

export const DEFAULT_VAULT_POLICY: VaultFleetPolicy = {
  schema: 3,
  ownerPrincipal: "",
  diskNumber: null,
  diskUniqueId: "",
  confirmationText: "",
  mountScope: "per-user",
  allowUnallocatedSpace: true,
  unallocatedReserveMb: 256,
  preloadDriverAtStartup: true,
  installDirectory: "C:\\Program Files\\WinCommander\\VeraCrypt Engine",
  volumes: [],
};

const STORAGE_KEY = "wincommander.fleet.vault-policy.v1";

export function loadVaultPolicy(): VaultFleetPolicy {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (!stored) return DEFAULT_VAULT_POLICY;
    const parsed = JSON.parse(stored) as VaultFleetPolicy | {
      schema: 1 | 2;
      groups?: unknown;
      volumes?: Array<VaultVolumePolicy | LegacyVaultVolume>;
    };
    if (parsed.schema !== 1 && parsed.schema !== 2 && parsed.schema !== 3) return DEFAULT_VAULT_POLICY;
    const legacy = parsed as typeof parsed & { groups?: unknown };
    const vault = { ...legacy };
    delete vault.groups;
    const volumes = (legacy.volumes ?? []).map(volume => {
      if ("groupPermissions" in volume && volume.groupPermissions) return volume as VaultVolumePolicy;
      const legacyVolume = volume as LegacyVaultVolume;
      const groupPermissions = Object.fromEntries((legacyVolume.allowedGroups ?? []).map(groupId => [
        groupId,
        legacyVolume.groupReadOnly ? "read" : "write",
      ]));
      const migrated = { ...legacyVolume, groupPermissions };
      delete migrated.allowedGroups;
      delete migrated.groupReadOnly;
      return migrated as VaultVolumePolicy;
    });
    return { ...DEFAULT_VAULT_POLICY, ...vault, volumes, schema: 3 } as VaultFleetPolicy;
  } catch {
    return DEFAULT_VAULT_POLICY;
  }
}

export function saveVaultPolicy(policy: VaultFleetPolicy) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(policy));
}

export function validateVaultPolicy(policy: VaultFleetPolicy, groups: FleetAccessGroup[]): string[] {
  const errors: string[] = [];
  if (!policy.ownerPrincipal.trim()) errors.push("Owner identity is required.");
  if (policy.diskNumber == null || !policy.diskUniqueId) errors.push("Select and pin a test disk before deployment.");
  if (policy.volumes.length === 0) errors.push("Add at least one Vault volume.");
  if (policy.volumes.some(volume => !volume.driveLetter.match(/^[A-Z]$/))) errors.push("Every volume needs one A-Z drive letter.");
  if (policy.volumes.some(volume => volume.driveLetter === "C" || volume.driveLetter === "D")) errors.push("C: and D: are protected and cannot be assigned to a Vault volume.");
  if (new Set(policy.volumes.map(volume => volume.driveLetter)).size !== policy.volumes.length) errors.push("Drive letters must be unique.");
  const deviceDiskPattern = /harddisk(\d+)/i;
  if (policy.diskNumber != null && policy.volumes.some(volume => {
    const match = volume.kind === "partition" ? volume.backing.match(deviceDiskPattern) : null;
    return match && Number(match[1]) !== policy.diskNumber;
  })) errors.push("Every raw device path must belong to the pinned test disk.");
  const groupIds = new Set(groups.map(group => group.id));
  if (policy.volumes.some(volume => Object.keys(volume.groupPermissions).some(groupId => !groupIds.has(groupId)))) {
    errors.push("A Vault volume refers to a group that no longer exists.");
  }
  return errors;
}

export function buildVaultMatrix(
  policy: VaultFleetPolicy,
  directory: FleetAccessDirectory,
): VaultMatrixRow[] {
  const principals = [
    { userClass: "Owner / administrator", groupIds: [] as string[], groups: "All Vault volumes", owner: true },
    ...directory.users.map(user => {
      const memberships = directory.groups.filter(group => group.userIds.includes(user.id));
      return {
        userClass: user.displayName || user.username,
        groupIds: memberships.map(group => group.id),
        groups: memberships.map(group => group.name).join(", ") || "Unassigned",
        owner: false,
      };
    }),
  ];

  return policy.volumes.flatMap(volume => principals.map(principal => {
    const resolution = principal.owner ? undefined : resolveGroupPolicy(
      principal.groupIds,
      directory.groups.map(group => ({
        groupId: group.id,
        value: !volume.ownerOnly ? volume.groupPermissions[group.id] ?? "none" : "none",
      })),
      { mode: "ranked", order: ["none", "read", "write"] },
    );
    const access = principal.owner ? "write" : resolution?.value ?? "none";
    const allowed = access !== "none";
    return {
      userClass: principal.userClass,
      groups: principal.groups,
      volume: volume.label,
      effectiveAccess: access === "write" ? "Write" : access === "read" ? "Read" : "None",
      canSeeBacking: allowed,
      canMount: allowed,
      canDecrypt: allowed,
      canAccessContent: allowed,
      seesOtherSessionMount: principal.owner,
    };
  }));
}

export function updateVolume(
  volumes: VaultVolumePolicy[],
  id: string,
  patch: Partial<VaultVolumePolicy>,
) {
  return volumes.map(volume => volume.id === id ? { ...volume, ...patch } : volume);
}
