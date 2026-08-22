import { describe, expect, test } from "bun:test";
import { newVaultPolicy } from "./vaultAccessTypes";
import {
  clearVaultAccessDraft,
  readVaultAccessDraft,
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
});
