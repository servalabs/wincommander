import type { VaultAccessEntry } from "./vaultAccessTypes";

const LEGACY_VAULT_STORAGE_KEY = "wincommander.fleet.vault-policy.v1";

/**
 * Reads the retired browser planner only after an administrator explicitly
 * asks to inspect it. The result is a draft, never active service policy.
 */
export function readUntrustedLegacyVaultDraft(): VaultAccessEntry[] | null {
  try {
    const raw = localStorage.getItem(LEGACY_VAULT_STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as { ownerPrincipal?: unknown; volumes?: unknown };
    if (!Array.isArray(parsed.volumes)) return null;
    const owner = typeof parsed.ownerPrincipal === "string" && parsed.ownerPrincipal.trim()
      ? parsed.ownerPrincipal : "Administrator";
    return parsed.volumes.flatMap((volume, index) => {
      if (!volume || typeof volume !== "object") return [];
      const legacy = volume as { id?: unknown; label?: unknown; backing?: unknown; driveLetter?: unknown; groupPermissions?: unknown; ownerOnly?: unknown };
      const grants = legacy.groupPermissions && typeof legacy.groupPermissions === "object"
        ? Object.entries(legacy.groupPermissions).flatMap(([principal_name, access]) =>
          access === "read" || access === "write" ? [{ principal_name, access }] : [])
        : [];
      return [{
        id: typeof legacy.id === "string" && legacy.id ? legacy.id : `legacy-${index + 1}`,
        label: typeof legacy.label === "string" && legacy.label ? legacy.label : `Imported vault ${index + 1}`,
        // Old raw/partition backing paths are intentionally not carried into
        // the file-container policy. An admin must choose a valid container.
        container_path: "",
        owner_account: owner,
        grants: grants.length > 0 ? grants : [{ principal_name: owner, access: "write" }],
        mount: {
          presentation: legacy.ownerOnly === true ? "per-user" : "machine",
          preferred_letter: typeof legacy.driveLetter === "string" && /^[A-Z]$/i.test(legacy.driveLetter)
            ? legacy.driveLetter.toUpperCase() : undefined,
        },
      } satisfies VaultAccessEntry];
    });
  } catch {
    return null;
  }
}
