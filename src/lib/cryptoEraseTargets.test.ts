import { describe, it, expect } from "bun:test";
import {
  buildTargets,
  escrowRiskOf,
  requiresNuclear,
  type EncryptedTarget,
} from "./cryptoEraseTargets";
import type { BitLockerVolume } from "../hooks/useBackend";

const bl = (over: Partial<BitLockerVolume>): BitLockerVolume => ({
  mountPoint: "D:",
  volumeType: "Data",
  volumeStatus: "FullyEncrypted",
  encryptionMethod: "XtsAes256",
  protectorTypes: ["Tpm"],
  recoveryPasswordPresent: false,
  backupUsed: false,
  ...over,
});

describe("buildTargets", () => {
  it("merges veracrypt mounted volumes and bitlocker data volumes", () => {
    const targets = buildTargets(
      [{ letter: "V:", path: "D:\\vault.hc", type: "Mounted" }],
      [bl({ mountPoint: "D:" })],
      "C:",
    );
    const kinds = targets.map((t) => t.kind).sort();
    expect(kinds).toEqual(["bitlocker", "veracrypt"]);
  });

  it("marks the BitLocker OS volume as isOsVolume and requiring nuclear confirm", () => {
    const [os] = buildTargets([], [bl({ mountPoint: "C:", volumeType: "OperatingSystem" })], "C:");
    expect(os.isOsVolume).toBe(true);
    expect(requiresNuclear(os)).toBe(true);
  });

  it("a BitLocker data volume is not OS and not nuclear", () => {
    const [d] = buildTargets([], [bl({ mountPoint: "D:" })], "C:");
    expect(d.isOsVolume).toBe(false);
    expect(requiresNuclear(d)).toBe(false);
  });

  it("a veracrypt volume with no path is ineligible with a reason", () => {
    const [t]: EncryptedTarget[] = buildTargets([{ letter: "V:", path: null, type: "Mounted" }], [], "C:");
    expect(t.eligible).toBe(false);
    expect(t.reason).toBeTruthy();
  });

  it("carries escrowRisk onto BitLocker rows", () => {
    const [t] = buildTargets([], [bl({ recoveryPasswordPresent: true, backupUsed: true })], "C:");
    expect(t.escrowRisk).toBe(true);
  });

  it("a VeraCrypt raw device path is treated as an OS target (nuclear)", () => {
    const [t] = buildTargets([{ letter: "\\\\.\\C:", path: "\\\\.\\C:", type: "Mounted" }], [], "C:");
    expect(t.isOsVolume).toBe(true);
    expect(requiresNuclear(t)).toBe(true);
  });
});

describe("escrowRiskOf", () => {
  it("is true when a recovery password is present", () => {
    expect(escrowRiskOf(bl({ recoveryPasswordPresent: true }))).toBe(true);
  });
  it("is true when backupUsed", () => {
    expect(escrowRiskOf(bl({ backupUsed: true }))).toBe(true);
  });
  it("is false for a TPM-only volume", () => {
    expect(escrowRiskOf(bl({}))).toBe(false);
  });
});
