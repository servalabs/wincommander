import { describe, expect, test } from "bun:test";
import { buildVaultMatrix, DEFAULT_VAULT_POLICY, validateVaultPolicy } from "./vaultFleetPolicy";

const deployablePolicy = {
  ...DEFAULT_VAULT_POLICY,
  ownerPrincipal: "SERVER\\Parth",
  diskNumber: 1,
  diskUniqueId: "test-disk-id",
  confirmationText: "ERASE DISK 1",
};

describe("Fleet multi-user vault policy", () => {
  test("default topology contains the requested seven standard users", () => {
    expect(DEFAULT_VAULT_POLICY.groups.flatMap(group => group.users)).toHaveLength(7);
    expect(DEFAULT_VAULT_POLICY.groups.find(group => group.id === "accounting")?.users).toHaveLength(3);
    expect(DEFAULT_VAULT_POLICY.groups.find(group => group.id === "sales")?.users).toHaveLength(3);
    expect(DEFAULT_VAULT_POLICY.groups.find(group => group.id === "partner")?.users).toHaveLength(1);
  });

  test("requires a pinned disk but accepts the complete requested topology", () => {
    expect(validateVaultPolicy(DEFAULT_VAULT_POLICY)).toContain("Select and pin a test disk before deployment.");
    expect(validateVaultPolicy(deployablePolicy)).toEqual([]);
  });

  test("fails closed on protected drive letters and a raw path on another disk", () => {
    const unsafe = {
      ...deployablePolicy,
      volumes: deployablePolicy.volumes.map((volume, index) => index === 0
        ? { ...volume, driveLetter: "C", kind: "partition" as const, backing: "\\\\Device\\Harddisk2\\Partition1" }
        : volume),
    };
    expect(validateVaultPolicy(unsafe)).toContain("C: and D: are protected and cannot be assigned to a Vault volume.");
    expect(validateVaultPolicy(unsafe)).toContain("Every raw device path must belong to the pinned test disk.");
  });

  test("matrix separates group access from owner and cross-session visibility", () => {
    const rows = buildVaultMatrix(deployablePolicy);
    const salesOwn = rows.find(row => row.userClass === "sales" && row.volume === "Sales");
    const accountingToSales = rows.find(row => row.userClass === "accounting" && row.volume === "Sales");
    const ownerToSales = rows.find(row => row.userClass === "owner" && row.volume === "Sales");
    expect(salesOwn).toMatchObject({ canMount: true, canDecrypt: true, canAccessContent: true, seesOtherSessionMount: false });
    expect(accountingToSales).toMatchObject({ canSeeBacking: false, canMount: false, canAccessContent: false });
    expect(ownerToSales).toMatchObject({ canMount: true, seesOtherSessionMount: true });
  });
});
