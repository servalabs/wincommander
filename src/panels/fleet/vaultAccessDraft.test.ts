import { describe, expect, test } from "bun:test";
import { newVaultPolicy } from "./vaultAccessTypes";
import {
  clearVaultAccessDraft,
  readVaultAccessDraft,
  readVaultAccessDraftSnapshot,
  prepareVaultAccessSave,
  vaultAccessEntryIntentEqual,
  rebaseVaultAccessDraft,
  writeVaultAccessDraft,
  type VaultDraftStorage,
} from "./vaultAccessDraft";

function memoryStorage(): VaultDraftStorage {
  const values = new Map<string, string>();
  return {
    getItem: key => values.get(key) ?? null,
    setItem: (key, value) => { values.set(key, value); },
    removeItem: key => { values.delete(key); },
  };
}

describe("Vault access draft persistence", () => {
  test("unchanged saved records compare equal across JSON property ordering", () => {
    const entry = newVaultPolicy().entries[0]!;
    const reordered = { ...entry, mount: { preferred_letter: "J", presentation: entry.mount.presentation } };
    const original = { mount: { presentation: entry.mount.presentation, preferred_letter: "J" },
      ...Object.fromEntries(Object.entries(entry).filter(([key]) => key !== "mount").reverse()) } as typeof entry;
    expect(vaultAccessEntryIntentEqual(original, reordered)).toBe(true);
    expect(vaultAccessEntryIntentEqual(original, { ...reordered, owner_account: "Changed" })).toBe(false);
  });

  test("saves a never-saved starter onto an existing service policy without replacing saved vaults", () => {
    const draft = newVaultPolicy();
    const saved = { ...newVaultPolicy(), version: 2, expected_previous_version: 1 };
    const prepared = prepareVaultAccessSave(draft, null, saved)!;
    expect(prepared.policy_id).toBe(saved.policy_id);
    expect(prepared.version).toBe(2);
    expect(prepared.entries).toEqual([...saved.entries, ...draft.entries]);
    expect(draft.version).toBe(0);
  });

  test("saves an added vault after another save advanced the service version", () => {
    const base = { ...newVaultPolicy(), version: 1 };
    const draft = { ...base, entries: [...base.entries, newVaultPolicy().entries[0]!] };
    const latest = { ...base, version: 3, entries: [...base.entries, newVaultPolicy().entries[0]!] };
    const prepared = prepareVaultAccessSave(draft, base, latest)!;
    expect(prepared.version).toBe(3);
    expect(prepared.entries).toEqual([...latest.entries, draft.entries.at(-1)!]);
  });

  test("does not overwrite a newer edit to the same vault", () => {
    const base = { ...newVaultPolicy(), version: 1 };
    const draft = { ...base, entries: [{ ...base.entries[0]!, label: "My edit" }] };
    const latest = { ...base, version: 2, entries: [{ ...base.entries[0]!, label: "Other edit" }] };
    expect(prepareVaultAccessSave(draft, base, latest)).toBeNull();
    expect(draft.entries[0]!.label).toBe("My edit");
  });

  test("path observations alone do not conflict with a vault edit", () => {
    const base = { ...newVaultPolicy(), version: 1 };
    const draft = { ...base, entries: base.entries.map(entry => ({ ...entry, label: "Edited" })) };
    const latest = { ...base, version: 2, entries: base.entries.map(entry => ({
      ...entry, container_path_state: "available" as const, canonical_container_path: entry.container_path,
    })) };
    expect(prepareVaultAccessSave(draft, base, latest)?.entries.every(entry => entry.label === "Edited")).toBe(true);
  });

  test("does not guess at a stale legacy draft or a replaced or removed policy", () => {
    const draft = { ...newVaultPolicy(), version: 1 };
    expect(prepareVaultAccessSave(draft, null, { ...draft, version: 2 })).toBeNull();
    expect(prepareVaultAccessSave(draft, draft, { ...draft, policy_id: "replacement" })).toBeNull();
    expect(prepareVaultAccessSave(draft, draft, null)).toBeNull();
    expect(prepareVaultAccessSave(draft, null, draft)).toEqual(draft);
  });

  test("refuses a starter ID collision and keeps first-ever creation at revision zero", () => {
    const draft = newVaultPolicy();
    expect(prepareVaultAccessSave(draft, null, { ...draft, version: 1 })).toBeNull();
    expect(prepareVaultAccessSave(draft, null, null)).toEqual(draft);
  });

  test("keeps a renderer draft until the user clears it", () => {
    const storage = memoryStorage();
    const policy = newVaultPolicy();
    policy.entries[0]!.container_path = "D:\\Windows\\pagefile.sy";
    policy.entries[0]!.container_identity = null;
    policy.entries[0]!.mount.preferred_letter = null;

    writeVaultAccessDraft(policy, storage);
    expect(readVaultAccessDraft(storage)).toEqual(policy);

    clearVaultAccessDraft(storage);
    expect(readVaultAccessDraft(storage)).toBeNull();
  });

  test("round-trips the exact shared access selector choice", () => {
    const storage = memoryStorage();
    const policy = newVaultPolicy();
    const entry = policy.entries[0]!;
    entry.owner_account = "PC\\Owner";
    entry.grants = [{ principal_name: "PC\\Owner", access: "write" }];
    entry.access_pattern = "shared-write";

    writeVaultAccessDraft(policy, storage);
    expect(readVaultAccessDraft(storage)?.entries[0]?.access_pattern).toBe("shared-write");
  });

  test("rejects malformed local data instead of treating it as policy", () => {
    const storage = memoryStorage();
    storage.setItem("wincommander.vault-access-draft.v1", JSON.stringify({ schema_version: 1, entries: "bad" }));
    expect(readVaultAccessDraft(storage)).toBeNull();
  });

  test("upgrades older drafts to a standard container without inventing a secret", () => {
    const storage = memoryStorage();
    const policy = newVaultPolicy();
    const legacy = structuredClone(policy) as unknown as { entries: Array<Record<string, unknown>> };
    for (const entry of legacy.entries) delete entry.container_kind;
    storage.setItem("wincommander.vault-access-draft.v1", JSON.stringify(legacy));

    expect(readVaultAccessDraft(storage)?.entries.every(entry => entry.container_kind === "standard")).toBe(true);
  });

  test("migrates the short-lived volume_kind draft field to the service wire name", () => {
    const storage = memoryStorage();
    const policy = newVaultPolicy();
    const legacy = structuredClone(policy) as unknown as { entries: Array<Record<string, unknown>> };
    legacy.entries[0]!.volume_kind = "dual";
    delete legacy.entries[0]!.container_kind;
    storage.setItem("wincommander.vault-access-draft.v1", JSON.stringify(legacy));

    const restored = readVaultAccessDraft(storage)!;
    expect(restored.entries[0]!.container_kind).toBe("dual");
    expect("volume_kind" in restored.entries[0]!).toBe(false);
  });

  test("persists the saved base snapshot needed for a safe rebase", () => {
    const storage = memoryStorage();
    const base = newVaultPolicy();
    const draft = { ...base, entries: base.entries.map((entry, index) => index === 0 ? { ...entry, label: "Local label" } : entry) };

    writeVaultAccessDraft(draft, storage, base);
    expect(readVaultAccessDraftSnapshot(storage)).toEqual({ policy: draft, basePolicy: base });
  });

  test("rebases a local vault edit while preserving a separately-added saved vault", () => {
    const base = newVaultPolicy();
    const draft = { ...base, entries: base.entries.map((entry, index) => index === 0 ? { ...entry, label: "Local label" } : entry) };
    const saved = { ...base, version: 4, expected_previous_version: 4, entries: [...base.entries, { ...base.entries[0]!, id: "server-added", label: "Server vault" }] };

    const rebased = rebaseVaultAccessDraft(draft, base, saved);
    expect(rebased?.version).toBe(4);
    expect(rebased?.entries.find(entry => entry.id === base.entries[0]?.id)?.label).toBe("Local label");
    expect(rebased?.entries.find(entry => entry.id === "server-added")?.label).toBe("Server vault");
  });

  test("refuses a rebase when the same vault changed in both drafts", () => {
    const base = newVaultPolicy();
    const draft = { ...base, entries: base.entries.map((entry, index) => index === 0 ? { ...entry, label: "Local label" } : entry) };
    const saved = { ...base, version: 2, entries: base.entries.map((entry, index) => index === 0 ? { ...entry, label: "Saved label" } : entry) };

    expect(rebaseVaultAccessDraft(draft, base, saved)).toBeNull();
  });
});
