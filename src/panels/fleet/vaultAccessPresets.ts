import type { VaultAccessEntry } from "./vaultAccessTypes";

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
  const owner = entry.owner_account.trim().toLocaleLowerCase();
  const isOwnerGrant = (principalName: string) => owner.length > 0
    && principalName.trim().toLocaleLowerCase() === owner;
  const ownerHasWriteGrant = entry.grants.some(grant => isOwnerGrant(grant.principal_name) && grant.access === "write");
  if (entry.mount.presentation === "per-user" && hasOwnerOnlyWriteGrant) return "private";
  // A view-only shared Vault still needs one accountable owner who can
  // maintain its contents. Everyone else starts read-only; deliberately
  // changing an individual row to write makes the policy custom instead of
  // silently turning every reader into an editor.
  if (entry.mount.presentation === "machine" && entry.grants.length > 0
    && ownerHasWriteGrant
    && entry.grants.every(grant => isOwnerGrant(grant.principal_name) || grant.access === "read")) return "shared-read";
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

  const owner = entry.owner_account.trim();
  const ownerKey = owner.toLocaleLowerCase();
  const containsOwner = entry.grants.some(grant => grant.principal_name.trim().toLocaleLowerCase() === ownerKey);
  const grants: VaultAccessEntry["grants"] = entry.grants.length > 0
    ? entry.grants.map(grant => ({
      ...grant,
      access: preset === "shared-read"
        ? (grant.principal_name.trim().toLocaleLowerCase() === ownerKey ? "write" : "read")
        : "write",
    }))
    : [];
  // Shared policies must state the owner's write grant explicitly. This also
  // lets a saved owner-only starter become a view-only share safely.
  if (owner && !containsOwner) grants.unshift({ principal_name: owner, access: "write" });
  return {
    ...entry,
    grants,
    mount: { ...entry.mount, presentation: "machine" },
  };
}
