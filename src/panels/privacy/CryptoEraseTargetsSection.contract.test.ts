import { readFileSync } from "node:fs";
import { describe, expect, it } from "bun:test";

const source = readFileSync("src/panels/privacy/CryptoEraseTargetsSection.tsx", "utf8");

describe("VeraCrypt lockdown target safety contract", () => {
  it("never treats broad creation candidates as verified VeraCrypt partitions", () => {
    expect(source).not.toContain("getEncryptionPartitions");
    expect(source).not.toContain("safeForCreation");
    expect(source).not.toContain("EncryptionPartition");
  });

  it("keeps explicitly persisted raw targets visible without relying on discovery", () => {
    expect(source).toContain("veracryptDevices.map");
    expect(source).toContain("targets remain visible while disconnected");
  });

  it("explains the fail-closed empty state instead of suggesting ordinary disks", () => {
    expect(source).toContain(
      "No VeraCrypt partitions enrolled — ordinary disks are intentionally not suggested.",
    );
  });
});
