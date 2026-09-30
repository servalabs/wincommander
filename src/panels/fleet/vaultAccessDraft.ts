import { normalizeVaultAccessPolicy, type VaultAccessEntry, type VaultAccessPolicy } from "./vaultAccessTypes";

const VAULT_ACCESS_DRAFT_KEY = "wincommander.vault-access-draft.v1";

export interface VaultDraftStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

export interface VaultAccessDraftSnapshot {
  policy: VaultAccessPolicy;
  /** The last service policy the draft was based on. This makes a three-way
   * rebase possible without treating a local browser copy as authoritative. */
  basePolicy: VaultAccessPolicy | null;
}

function isVaultAccessPolicy(value: unknown): value is VaultAccessPolicy {
  if (!value || typeof value !== "object") return false;
  const policy = value as Partial<VaultAccessPolicy>;
  if (
    policy.schema_version !== 1
    || typeof policy.policy_id !== "string"
    || typeof policy.version !== "number"
    || typeof policy.expected_previous_version !== "number"
    || !Array.isArray(policy.entries)
  ) return false;

  return policy.entries.every(entry => {
    const legacy = entry as VaultAccessEntry & { volume_kind?: unknown };
    const containerKind = legacy.container_kind ?? legacy.volume_kind;
    return !!entry
    && typeof entry.id === "string"
    && typeof entry.label === "string"
    && typeof entry.container_path === "string"
    && (containerKind === undefined || containerKind === "standard" || containerKind === "dual")
    && (entry.container_identity === undefined || entry.container_identity === null || typeof entry.container_identity === "string")
    && typeof entry.owner_account === "string"
    && (entry.access_pattern === undefined || entry.access_pattern === null || entry.access_pattern === "private" || entry.access_pattern === "shared-read" || entry.access_pattern === "shared-write")
    && Array.isArray(entry.grants)
    && entry.grants.every(grant => (
      !!grant
      && typeof grant.principal_name === "string"
      && (grant.access === "read" || grant.access === "write")
    ))
    && !!entry.mount
    && (entry.mount.presentation === "machine" || entry.mount.presentation === "per-user")
    && (entry.mount.preferred_letter === undefined || entry.mount.preferred_letter === null || typeof entry.mount.preferred_letter === "string");
  });
}

function browserStorage(): VaultDraftStorage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

function clonePolicy(policy: VaultAccessPolicy): VaultAccessPolicy {
  return JSON.parse(JSON.stringify(policy)) as VaultAccessPolicy;
}

export function vaultAccessEntryIntentEqual(left: VaultAccessEntry, right: VaultAccessEntry): boolean {
  // These are service observations/capabilities, not administrator edits.
  // Normalising also makes a legacy null access_pattern compare the same as
  // the omitted field emitted by current drafts.
  const intent = (entry: VaultAccessEntry) => {
    const normalized = normalizeVaultAccessPolicy({
      schema_version: 1, policy_id: "intent", version: 0, expected_previous_version: 0, entries: [entry],
    }).entries[0]!;
    const {
      container_path_state: _state,
      canonical_container_path: _canonical,
      container_identity: _identity,
      can_edit_policy: _canEdit,
      can_remove_policy: _canRemove,
      ...requestIntent
    } = normalized;
    return requestIntent;
  };
  const leftIntent = intent(left);
  const rightIntent = intent(right);
  const orderedFields = (_key: string, value: unknown) => value && typeof value === "object" && !Array.isArray(value)
    ? Object.fromEntries(Object.entries(value).sort(([leftKey], [rightKey]) => leftKey.localeCompare(rightKey)))
    : value;
  return JSON.stringify(leftIntent, orderedFields) === JSON.stringify(rightIntent, orderedFields);
}

/** Whether this row is new or has a real policy edit relative to the newest
 * service snapshot. Unchanged rows are omitted from an owner fragment: that
 * prevents a currently mounted, unrelated Vault from failing a new save's
 * drive-letter preflight. The service still validates every submitted edit. */
export function vaultAccessEntryChangedSince(entry: VaultAccessEntry, savedPolicy: VaultAccessPolicy | null): boolean {
  return !savedPolicy?.entries.some(saved => saved.id === entry.id && vaultAccessEntryIntentEqual(saved, entry));
}

/**
 * Renderer drafts contain paths and account names only. They never contain a
 * mount secret and never count as applied policy; the SYSTEM service still
 * resolves and validates every field when the administrator applies it.
 */
export function readVaultAccessDraft(storage: VaultDraftStorage | null = browserStorage()): VaultAccessPolicy | null {
  return readVaultAccessDraftSnapshot(storage)?.policy ?? null;
}

export function readVaultAccessDraftSnapshot(storage: VaultDraftStorage | null = browserStorage()): VaultAccessDraftSnapshot | null {
  if (!storage) return null;
  try {
    const raw = storage.getItem(VAULT_ACCESS_DRAFT_KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    // v1 persisted the policy directly. Keep it readable, but it has no
    // trusted base snapshot and therefore cannot be automatically rebased.
    if (isVaultAccessPolicy(parsed)) return { policy: normalizeVaultAccessPolicy(parsed), basePolicy: null };
    if (!parsed || typeof parsed !== "object") return null;
    const record = parsed as { policy?: unknown; basePolicy?: unknown };
    return isVaultAccessPolicy(record.policy)
      ? {
        policy: normalizeVaultAccessPolicy(record.policy),
        basePolicy: isVaultAccessPolicy(record.basePolicy) ? normalizeVaultAccessPolicy(record.basePolicy) : null,
      }
      : null;
  } catch {
    return null;
  }
}

export function writeVaultAccessDraft(
  policy: VaultAccessPolicy,
  storage: VaultDraftStorage | null = browserStorage(),
  basePolicy: VaultAccessPolicy | null = null,
): void {
  if (!storage) return;
  try {
    storage.setItem(VAULT_ACCESS_DRAFT_KEY, JSON.stringify({ policy, basePolicy }));
  } catch {
    // A blocked/full browser store must not prevent editing or applying.
  }
}

/**
 * Safe three-way policy rebase. It carries a local addition, deletion, or an
 * unchanged-server entry forward, but refuses to guess when both sides edited
 * the same vault. In that case the caller keeps the draft and asks the admin
 * to reload/review rather than silently overwriting newer access rules.
 */
export function rebaseVaultAccessDraft(
  draft: VaultAccessPolicy,
  basePolicy: VaultAccessPolicy | null,
  savedPolicy: VaultAccessPolicy | null,
): VaultAccessPolicy | null {
  if (!basePolicy || !savedPolicy || draft.policy_id !== basePolicy.policy_id || basePolicy.policy_id !== savedPolicy.policy_id) {
    return null;
  }
  const base = new Map(basePolicy.entries.map(entry => [entry.id, entry]));
  const draftEntries = new Map(draft.entries.map(entry => [entry.id, entry]));
  const saved = new Map(savedPolicy.entries.map(entry => [entry.id, entry]));
  const same = vaultAccessEntryIntentEqual;
  const entries = [...savedPolicy.entries];

  for (const [id, baseEntry] of base) {
    const draftEntry = draftEntries.get(id);
    const savedEntry = saved.get(id);
    if (!draftEntry) {
      if (savedEntry && !same(savedEntry, baseEntry)) return null;
      const index = entries.findIndex(entry => entry.id === id);
      if (index >= 0) entries.splice(index, 1);
      continue;
    }
    if (!savedEntry) {
      if (!same(draftEntry, baseEntry)) return null;
      continue;
    }
    if (same(draftEntry, baseEntry) || same(draftEntry, savedEntry)) continue;
    if (!same(savedEntry, baseEntry)) return null;
    const index = entries.findIndex(entry => entry.id === id);
    entries[index] = clonePolicy({ ...savedPolicy, entries: [draftEntry] }).entries[0]!;
  }
  for (const [id, draftEntry] of draftEntries) {
    if (base.has(id)) continue;
    if (saved.has(id)) return null;
    entries.push(clonePolicy({ ...draft, entries: [draftEntry] }).entries[0]!);
  }
  return {
    ...clonePolicy(savedPolicy),
    version: savedPolicy.version,
    expected_previous_version: savedPolicy.version,
    entries,
  };
}

export function clearVaultAccessDraft(storage: VaultDraftStorage | null = browserStorage()): void {
  if (!storage) return;
  try {
    storage.removeItem(VAULT_ACCESS_DRAFT_KEY);
  } catch {
    // Applying remains service-owned even when local cleanup is blocked.
  }
}

/** Prepare one save against a fresh service read without overwriting concurrent edits. */
export function prepareVaultAccessSave(
  draft: VaultAccessPolicy,
  basePolicy: VaultAccessPolicy | null,
  savedPolicy: VaultAccessPolicy | null,
): VaultAccessPolicy | null {
  if (!savedPolicy) {
    if (basePolicy || draft.version !== 0) return null;
    return { ...clonePolicy(draft), version: 0, expected_previous_version: 0 };
  }
  if (basePolicy) return rebaseVaultAccessDraft(draft, basePolicy, savedPolicy);
  if (draft.version === 0 && draft.expected_previous_version === 0) {
    // An unsaved starter has no saved rows to replace or delete.
    const savedIds = new Set(savedPolicy.entries.map(entry => entry.id));
    if (draft.entries.some(entry => savedIds.has(entry.id))) return null;
    return {
      ...clonePolicy(savedPolicy),
      expected_previous_version: savedPolicy.version,
      entries: [...clonePolicy(savedPolicy).entries, ...clonePolicy(draft).entries],
    };
  }
  // Older persisted drafts have no base for a three-way comparison.
  if (draft.policy_id !== savedPolicy.policy_id || draft.version !== savedPolicy.version) return null;
  return clonePolicy(draft);
}

/** Keep unrelated edits without turning a concurrent addition into a later deletion. */
export function retainVaultDraftAfterSave(
  draft: VaultAccessPolicy,
  base: VaultAccessPolicy | null,
  saved: VaultAccessPolicy,
  removedIds: readonly string[],
): VaultAccessDraftSnapshot {
  if (!saved.entries.length) return {
    policy: { ...clonePolicy(draft), version: 0, expected_previous_version: 0 }, basePolicy: null,
  };
  const remainingBase = base ? { ...base, entries: base.entries.filter(entry => !removedIds.includes(entry.id)) } : null;
  const rebased = rebaseVaultAccessDraft(draft, remainingBase, saved);
  return { policy: rebased ?? draft, basePolicy: rebased ? saved : remainingBase };
}
