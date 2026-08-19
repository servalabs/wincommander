import type {
  FleetAccessDirectory, FleetAccessGroup, FleetAccessUser,
} from "./accessControlTypes";

export const ACCESS_CONTROL_STORAGE_KEY = "wincommander.fleet.access-control.v1";
const LEGACY_VAULT_STORAGE_KEY = "wincommander.fleet.vault-policy.v1";

export const DEFAULT_ACCESS_DIRECTORY: FleetAccessDirectory = {
  schema: 1,
  users: [],
  groups: [],
};

interface LegacyVaultGroup {
  id: string;
  localGroup: string;
  users: string[];
}

function userId(username: string) {
  return username.trim().toLocaleLowerCase();
}

export function mergeAccessUsers(
  existing: FleetAccessUser[],
  discovered: FleetAccessUser[],
): FleetAccessUser[] {
  const byId = new Map(existing.map(user => [userId(user.username), user]));
  for (const user of discovered) {
    const id = userId(user.username);
    if (!id) continue;
    byId.set(id, { ...byId.get(id), ...user, id });
  }
  return [...byId.values()].sort((left, right) =>
    left.username.localeCompare(right.username, undefined, { sensitivity: "base" }));
}

function migrateLegacyGroups(groups: LegacyVaultGroup[]): FleetAccessDirectory {
  const users = mergeAccessUsers([], groups.flatMap(group => group.users.map(username => ({
    id: userId(username),
    username,
  }))));
  return {
    schema: 1,
    users,
    groups: groups.map(group => ({
      id: group.id,
      name: group.id.replace(/(^|[-_])([a-z])/g, (_, prefix: string, letter: string) =>
        `${prefix ? " " : ""}${letter.toUpperCase()}`),
      localGroup: group.localGroup,
      userIds: group.users.map(userId),
    })),
  };
}

export function loadAccessDirectory(): FleetAccessDirectory {
  try {
    const stored = localStorage.getItem(ACCESS_CONTROL_STORAGE_KEY);
    if (stored) {
      const parsed = JSON.parse(stored) as FleetAccessDirectory;
      if (parsed.schema === 1 && Array.isArray(parsed.users) && Array.isArray(parsed.groups)) {
        return parsed;
      }
    }

    const legacy = localStorage.getItem(LEGACY_VAULT_STORAGE_KEY);
    if (legacy) {
      const parsed = JSON.parse(legacy) as { groups?: LegacyVaultGroup[] };
      if (Array.isArray(parsed.groups)) return migrateLegacyGroups(parsed.groups);
    }
  } catch {
    // A damaged local planner file must not invent access assignments.
  }
  return DEFAULT_ACCESS_DIRECTORY;
}

export function saveAccessDirectory(directory: FleetAccessDirectory) {
  localStorage.setItem(ACCESS_CONTROL_STORAGE_KEY, JSON.stringify(directory));
}

export function validateAccessDirectory(directory: FleetAccessDirectory): string[] {
  const errors: string[] = [];
  const userIds = new Set(directory.users.map(user => user.id));
  const groupNames = directory.groups.map(group => group.name.trim().toLocaleLowerCase());
  const localGroups = directory.groups.map(group => group.localGroup.trim().toLocaleLowerCase());

  if (directory.groups.some(group => !group.name.trim())) errors.push("Every group needs a name.");
  if (directory.groups.some(group => !group.localGroup.trim())) errors.push("Every group needs a Windows group name.");
  if (new Set(groupNames).size !== groupNames.length) errors.push("Group names must be unique.");
  if (new Set(localGroups).size !== localGroups.length) errors.push("Windows group names must be unique.");
  if (directory.groups.some(group => new Set(group.userIds).size !== group.userIds.length)) {
    errors.push("A user cannot be listed twice inside the same group.");
  }
  if (directory.groups.some(group => group.userIds.some(id => !userIds.has(id)))) {
    errors.push("A group contains a Windows user that is no longer available.");
  }
  return errors;
}

export function createAccessGroup(groups: FleetAccessGroup[]): FleetAccessGroup {
  const id = typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : `group-${Date.now().toString(36)}`;
  let index = groups.length + 1;
  let name = `New group ${index}`;
  while (groups.some(group => group.name.toLocaleLowerCase() === name.toLocaleLowerCase())) {
    index += 1;
    name = `New group ${index}`;
  }
  return { id, name, localGroup: `WC_Group_${index}`, userIds: [] };
}

export function membershipCount(groups: FleetAccessGroup[], userIdToFind: string) {
  return groups.reduce((count, group) => count + Number(group.userIds.includes(userIdToFind)), 0);
}
