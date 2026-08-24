import { describe, expect, test } from "bun:test";
import { newVaultPolicy } from "./vaultAccessTypes";
import {
  clearVaultAccessDraft,
  readVaultAccessDraft,
  readVaultAccessDraftSnapshot,
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

  test("rejects malformed local data instead of treating it as policy", () => {
    const storage = memoryStorage();
    storage.setItem("wincommander.vault-access-draft.v1", JSON.stringify({ schema_version: 1, entries: "bad" }));
    expect(readVaultAccessDraft(storage)).toBeNull();
  });

  test("upgrades older drafts to a standard container without inventing a secret", () => {
    const storage = memoryStorage();
    const policy = newVaultPolicy();
    const legacy = structuredClone(policy) as { entries: Array<Record<string, unknown>> };
    for (const entry of legacy.entries) delete entry.volume_kind;
    storage.setItem("wincommander.vault-access-draft.v1", JSON.stringify(legacy));

    expect(readVaultAccessDraft(storage)?.entries.every(entry => entry.volume_kind === "standard")).toBe(true);
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
