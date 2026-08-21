import { describe, expect, test } from "bun:test";
import type { FleetAccessDirectory } from "./accessControlTypes";
import type { VaultFleetPolicy } from "./vaultFleetTypes";
import { buildVaultMatrix, DEFAULT_VAULT_POLICY, validateVaultPolicy } from "./vaultFleetPolicy";

const directory: FleetAccessDirectory = {
  schema: 1,
  users: [
    { id: "alex", username: "alex" },
    { id: "sam", username: "sam" },
  ],
  groups: [
    { id: "marketing", name: "Marketing", localGroup: "WC_Marketing", userIds: ["alex", "sam"] },
    { id: "developers", name: "Developers", localGroup: "WC_Developers", userIds: ["alex"] },
  ],
};

const deployablePolicy: VaultFleetPolicy = {
  ...DEFAULT_VAULT_POLICY,
  ownerPrincipal: "SERVER\\Parth",
  diskNumber: 1,
  diskUniqueId: "test-disk-id",
  confirmationText: "ERASE DISK 1",
  volumes: [
    {
      id: "marketing-docs",
      label: "Marketing documents",
      kind: "container" as const,
      backing: "marketing.hc",
      sizeMb: 1024,
      driveLetter: "M",
      credentialRef: "MarketingDocs",
      groupPermissions: { marketing: "read", developers: "write" },
      ownerOnly: false,
    },
    {
      id: "source-code",
      label: "Source code",
      kind: "container" as const,
      backing: "source.hc",
      sizeMb: 2048,
      driveLetter: "S",
      credentialRef: "SourceCode",
      groupPermissions: { developers: "write" },
      ownerOnly: false,
    },
  ],
};

describe("Fleet multi-user vault policy", () => {
  test("requires a pinned disk and a volume", () => {
    expect(validateVaultPolicy(DEFAULT_VAULT_POLICY, directory.groups)).toContain("Select and pin a test disk before deployment.");
    expect(validateVaultPolicy(DEFAULT_VAULT_POLICY, directory.groups)).toContain("Add at least one Vault volume.");
    expect(validateVaultPolicy(deployablePolicy, directory.groups)).toEqual([]);
  });

  test("fails closed on protected drive letters and a raw path on another disk", () => {
    const unsafe = {
      ...deployablePolicy,
      volumes: deployablePolicy.volumes.map((volume, index) => index === 0
        ? { ...volume, driveLetter: "C", kind: "partition" as const, backing: "\\\\Device\\Harddisk2\\Partition1" }
        : volume),
    };
    expect(validateVaultPolicy(unsafe, directory.groups)).toContain("C: and D: are protected and cannot be assigned to a Vault volume.");
    expect(validateVaultPolicy(unsafe, directory.groups)).toContain("Every raw device path must belong to the pinned test disk.");
  });

  test("rejects Vault assignments to a deleted group", () => {
    expect(validateVaultPolicy(deployablePolicy, directory.groups.filter(group => group.id !== "developers")))
      .toContain("A Vault volume refers to a group that no longer exists.");
  });

  test("a multi-group user receives the highest Vault grant", () => {
    const rows = buildVaultMatrix(deployablePolicy, directory);
    const alexMarketing = rows.find(row => row.userClass === "alex" && row.volume === "Marketing documents");
    const alexSource = rows.find(row => row.userClass === "alex" && row.volume === "Source code");
    const samSource = rows.find(row => row.userClass === "sam" && row.volume === "Source code");
    const samMarketing = rows.find(row => row.userClass === "sam" && row.volume === "Marketing documents");
    expect(alexMarketing).toMatchObject({ effectiveAccess: "Write", canMount: true });
    expect(samMarketing).toMatchObject({ effectiveAccess: "Read", canMount: true });
    expect(alexSource).toMatchObject({ effectiveAccess: "Write", canAccessContent: true });
    expect(samSource).toMatchObject({ effectiveAccess: "None", canSeeBacking: false });
  });
});
