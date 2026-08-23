import { describe, expect, test } from "bun:test";
import { importLegacyVaultDraft } from "./vaultLegacyImport";
import type { FleetAccessDirectory } from "./accessControlTypes";

const directory: FleetAccessDirectory = {
  schema: 1,
  users: [
    { id: "sid:s-1-owner", username: "PC\\Owner", sid: "S-1-owner", isCurrent: true, isAvailable: true },
    { id: "sid:s-1-reader", username: "PC\\Reader", sid: "S-1-reader", isAvailable: true },
    { id: "sid:s-1-gone", username: "PC\\Gone", sid: "S-1-gone", isAvailable: false },
  ],
  groups: [{ id: "finance", name: "Finance", localGroup: "WC_Finance", userIds: [] }],
};

describe("retired Vault planner import", () => {
  test("resolves known user SIDs and local group IDs before creating a draft", () => {
    const imported = importLegacyVaultDraft({
      ownerPrincipal: "S-1-owner",
      volumes: [{ id: "safe", groupPermissions: { finance: "read", "S-1-reader": "write" } }],
    }, directory);

    expect(imported?.entries[0]).toMatchObject({
      owner_account: "PC\\Owner",
      grants: [
        { principal_name: "WC_Finance", access: "read" },
        { principal_name: "PC\\Reader", access: "write" },
      ],
    });
    expect(imported?.droppedPrincipalCount).toBe(0);
  });

  test("drops unresolved or unavailable grants and skips a vault without a resolved owner", () => {
    const imported = importLegacyVaultDraft({
      ownerPrincipal: "missing-owner",
      volumes: [{ id: "unsafe", groupPermissions: { "S-1-gone": "write", unknown: "read" } }],
    }, directory);

    expect(imported).toMatchObject({ entries: [], skippedVolumeCount: 1, droppedPrincipalCount: 0 });

    const fallback = importLegacyVaultDraft({
      ownerPrincipal: "S-1-owner",
      volumes: [{ id: "review", groupPermissions: { "S-1-gone": "write", unknown: "read" } }],
    }, directory);
    expect(fallback?.entries[0]?.grants).toEqual([{ principal_name: "PC\\Owner", access: "write" }]);
    expect(fallback?.droppedPrincipalCount).toBe(2);
  });
});
