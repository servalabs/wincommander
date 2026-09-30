import { DEFAULT_ACCESS_DIRECTORY, fromVaultAccessDirectory, toVaultAccessDirectory } from "./accessControlPolicy";
import type { FleetAccessDirectory, VaultAccessDirectory, VaultSaveAccessDirectoryResponse } from "./accessControlTypes";

export function savedGroupSignature(directory: VaultAccessDirectory): string {
  return JSON.stringify(directory.groups.map(group => ({ id: group.id, name: group.name, local_group: group.local_group, member_sids: [...group.member_sids].map(sid => sid.toLowerCase()).sort() }))
    .sort((a, b) => a.id.localeCompare(b.id)));
}

function acceptedGroupsMatchIntent(requested: VaultAccessDirectory, accepted: VaultAccessDirectory,
  before: VaultAccessDirectory, creatorSid: string | undefined): boolean {
  // The service preserves IDs and names. Its sole group normalization is adding
  // the authenticated creator to NEW groups; never permit arbitrary extra SIDs,
  // silently omitted edits/deletions, or changes to existing group membership.
  const expected = { ...requested, groups: requested.groups.map(group => {
    const isNew = !before.groups.some(old => old.id.toLowerCase() === group.id.toLowerCase());
    if (!isNew || !creatorSid || group.member_sids.some(sid => sid.toLowerCase() === creatorSid.toLowerCase())) return group;
    return { ...group, member_sids: [...group.member_sids, creatorSid] };
  }) };
  return savedGroupSignature(expected) === savedGroupSignature(accepted);
}
function draftSignature(directory: FleetAccessDirectory): string {
  return JSON.stringify(directory.groups.map(group => ({ ...group, userIds: [...group.userIds].map(id => id.toLowerCase()).sort() }))
    .sort((a, b) => a.id.localeCompare(b.id)));
}
export interface AccessDirectoryState {
  directory: FleetAccessDirectory;
  loaded: boolean;
  loading: boolean;
  saving: boolean;
  dirty: boolean;
  error: string | null;
}
export function createAccessDirectoryController(
  read: () => Promise<VaultAccessDirectory>,
  write: (directory: VaultAccessDirectory) => Promise<VaultSaveAccessDirectoryResponse>,
  publish: (state: AccessDirectoryState) => void,
) {
  let state: AccessDirectoryState = { directory: DEFAULT_ACCESS_DIRECTORY, loaded: false, loading: false, saving: false, dirty: false, error: null };
  let savedSignature = draftSignature(state.directory);
  let savedWireSignature: string | null = null;
  let generation = 0;
  let edits = 0;
  const emit = (patch: Partial<AccessDirectoryState>) => { state = { ...state, ...patch }; publish(state); };
  const restore = (directory: VaultAccessDirectory) => {
    const restored = fromVaultAccessDirectory(directory);
    // Service records own groups. Fresh account-discovery flags are local
    // presentation only and may survive a group-directory refresh.
    restored.users = restored.users.map(user => {
      const previous = state.directory.users.find(candidate => candidate.sid?.toLowerCase() === user.sid?.toLowerCase());
      return previous ? { ...user, isCurrent: previous.isCurrent, isAvailable: previous.isAvailable } : user;
    });
    for (const discovered of state.directory.users) {
      if (discovered.sid && discovered.isAvailable === true
        && !restored.users.some(user => user.sid?.toLowerCase() === discovered.sid?.toLowerCase())) {
        restored.users.push(discovered);
      }
    }
    return restored;
  };
  return {
    getState: () => state,
    update(action: FleetAccessDirectory | ((current: FleetAccessDirectory) => FleetAccessDirectory)) {
      const directory = typeof action === "function" ? action(state.directory) : action;
      if (draftSignature(directory) !== draftSignature(state.directory)) edits++;
      emit({ directory, dirty: draftSignature(directory) !== savedSignature });
    },
    async refresh(discardDraft = false): Promise<boolean> {
      if (state.saving || (state.dirty && !discardDraft)) return false;
      const request = ++generation, revision = edits;
      emit({ loading: true, error: null });
      try {
        const directory = await read();
        if (request !== generation || revision !== edits) return false;
        const restored = restore(directory);
        savedSignature = draftSignature(restored);
        savedWireSignature = savedGroupSignature(directory);
        emit({ directory: restored, dirty: false, loaded: true });
        return true;
      } catch {
        if (request === generation) emit({ error: "Saved groups could not be read from this PC's security service. The previous list is preserved. Refresh groups to retry." });
        return false;
      } finally {
        if (request === generation) emit({ loading: false });
      }
    },
    async save(candidate: FleetAccessDirectory): Promise<VaultSaveAccessDirectoryResponse> {
      if (!state.loaded || state.saving) throw new Error("vault_group_directory_not_loaded");
      ++generation;
      const revision = edits;
      emit({ saving: true, loading: false, error: null });
      try {
        // Detect known stale drafts before sending any mutation. This is not
        // an atomic server CAS; the service remains the authorization owner.
        const before = await read().catch(() => { throw new Error("vault_group_refresh_required"); });
        if (savedGroupSignature(before) !== savedWireSignature) throw new Error("vault_group_refresh_required");
        const requested = toVaultAccessDirectory(candidate);
        const currentSids = [...new Set(candidate.users.filter(user => user.isCurrent && user.sid).map(user => user.sid!.toLowerCase()))];
        const creatorSid = currentSids.length === 1 ? currentSids[0] : undefined;
        const saved = await write(requested);
        if (!Array.isArray(saved.results) || saved.results.some(result => !["created", "updated", "unchanged"].includes(result.state))) {
          throw new Error("vault_group_readback_unconfirmed");
        }
        if (!acceptedGroupsMatchIntent(requested, saved.directory, before, creatorSid)) throw new Error("vault_group_readback_unconfirmed");
        const observed = await read().catch(() => { throw new Error("vault_group_readback_unconfirmed"); });
        if (savedGroupSignature(observed) !== savedGroupSignature(saved.directory)) throw new Error("vault_group_readback_unconfirmed");
        const restored = restore(observed);
        savedSignature = draftSignature(restored);
        savedWireSignature = savedGroupSignature(observed);
        emit(revision === edits
          ? { directory: restored, dirty: false }
          : { dirty: draftSignature(state.directory) !== savedSignature });
        return { ...saved, directory: observed };
      } finally {
        emit({ saving: false });
      }
    },
    invalidate() { generation++; },
  };
}
