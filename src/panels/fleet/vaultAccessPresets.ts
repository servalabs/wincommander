import type { VaultAccess, VaultAccessEntry } from "./vaultAccessTypes";

export type VaultAccessPreset = "private" | "shared-read" | "shared-write" | "custom";

export const VAULT_ACCESS_PRESETS: Record<Exclude<VaultAccessPreset, "custom">, {
  label: string;
  description: string;
}> = {
  private: {
    label: "Personal vault",
    description: "Only the owner can mount and change files. The drive appears only in that user's Windows session.",
  },
  "shared-read": {
    label: "Shared, view only",
    description: "Named people or groups share one drive. They can open and copy files, but cannot change them.",
  },
  "shared-write": {
    label: "Shared, can edit",
    description: "Named people or groups share one drive and can change files.",
  },
};

export function vaultAccessPreset(entry: VaultAccessEntry): VaultAccessPreset {
  const hasOwnerOnlyWriteGrant = entry.grants.length === 1
    && entry.grants[0]?.principal_name.trim().toLocaleLowerCase() === entry.owner_account.trim().toLocaleLowerCase()
    && entry.grants[0]?.access === "write";
  if (entry.mount.presentation === "per-user" && hasOwnerOnlyWriteGrant) return "private";
  if (entry.mount.presentation === "machine" && entry.grants.length > 0 && entry.grants.every(grant => grant.access === "read")) return "shared-read";
  if (entry.mount.presentation === "machine" && entry.grants.length > 0 && entry.grants.every(grant => grant.access === "write")) return "shared-write";
  return "custom";
}

export function applyVaultAccessPreset(
  entry: VaultAccessEntry,
  preset: Exclude<VaultAccessPreset, "custom">,
): VaultAccessEntry {
  if (preset === "private") {
    return {
      ...entry,
      grants: [{ principal_name: entry.owner_account, access: "write" }],
      mount: { ...entry.mount, presentation: "per-user" },
    };
  }

  const access: VaultAccess = preset === "shared-read" ? "read" : "write";
  const grants = entry.grants.length > 0
    ? entry.grants.map(grant => ({ ...grant, access }))
    : [{ principal_name: entry.owner_account, access }];
  return {
    ...entry,
    grants,
    mount: { ...entry.mount, presentation: "machine" },
  };
}
