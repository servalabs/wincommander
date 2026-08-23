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

function sidId(sid: string | undefined) {
  const normalized = sid?.trim().toLocaleLowerCase();
  return normalized ? `sid:${normalized}` : null;
}

function canonicalUserId(user: FleetAccessUser) {
  return sidId(user.sid) ?? (user.id.trim() || userId(user.username));
}

/** Reconcile renamed Windows accounts by SID and carry their group memberships forward. */
export function reconcileAccessDirectoryUsers(
  directory: FleetAccessDirectory,
  discovered: FleetAccessUser[],
): FleetAccessDirectory {
  const groups = new Map<string, FleetAccessUser[]>();
  const withoutSid: FleetAccessUser[] = [];
  const sources = [...directory.users, ...discovered];

  // SID-bearing rows are authoritative. Group them before considering legacy
  // name-only records so two accounts that reused a display/login name can
  // never be bridged through that older record.
  for (const user of sources) {
    if (!userId(user.username)) continue;
    const sid = sidId(user.sid);
    if (sid) groups.set(sid, [...(groups.get(sid) ?? []), user]);
    else withoutSid.push(user);
  }

  for (const user of withoutSid) {
    const sameNameSidGroups = [...groups.entries()].filter(([, candidates]) =>
      candidates.some(candidate => userId(candidate.username) === userId(user.username)));
    // A legacy name-only row may adopt exactly one discovered identity. More
    // than one SID is ambiguous, so preserve it as unavailable history rather
    // than merging memberships into either account.
    const key = sameNameSidGroups.length === 1
      ? sameNameSidGroups[0]![0]
      : `legacy:${user.id.trim() || userId(user.username)}`;
    groups.set(key, [...(groups.get(key) ?? []), user]);
  }

  const remappedIds = new Map<string, string>();
  const users = [...groups.values()].map(group => {
    const preferred = [...group].reverse().find(user => discovered.includes(user)) ?? group[group.length - 1];
    // Do not let a partial discovery row overwrite a persisted SID with
    // undefined. A SID is the only stable account identity across renames.
    const merged = Object.assign({}, ...group, preferred) as FleetAccessUser;
    const stableSid = group.map(user => user.sid?.trim()).find(Boolean);
    if (stableSid) merged.sid = stableSid;
    const id = canonicalUserId(merged);
    for (const source of group) remappedIds.set(source.id, id);
    return { ...merged, id, isAvailable: group.some(user => discovered.includes(user)) };
  }).sort((left, right) =>
    left.username.localeCompare(right.username, undefined, { sensitivity: "base" }));

  const groupsWithStableMemberships = directory.groups.map(group => ({
    ...group,
    userIds: [...new Set(group.userIds.map(id => remappedIds.get(id) ?? id))],
  }));

  return { ...directory, users, groups: groupsWithStableMemberships };
}

export function mergeAccessUsers(
  existing: FleetAccessUser[],
  discovered: FleetAccessUser[],
): FleetAccessUser[] {
  return reconcileAccessDirectoryUsers(
    { schema: 1, users: existing, groups: [] },
    discovered,
  ).users;
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
  if (directory.groups.some(group => group.userIds.some(id =>
    directory.users.find(user => user.id === id)?.isAvailable === false))) {
    errors.push("A group contains a Windows account that is disabled or deleted.");
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
