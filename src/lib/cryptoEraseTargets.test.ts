import { describe, it, expect } from "bun:test";
import {
  buildTargets,
  deriveSystemDrive,
  eraseConsequences,
  eraseLimitation,
  eraseMethodLabel,
  escrowRiskOf,
  expectedAckToken,
  requiresNuclear,
  targetSubject,
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

describe("buildTargets eligibility", () => {
  it("refuses a fully decrypted BitLocker volume — there are no keys to destroy", () => {
    const [t] = buildTargets([], [bl({ volumeStatus: "FullyDecrypted" })], "C:");
    expect(t.isEncrypted).toBe(false);
    expect(t.eligible).toBe(false);
    expect(t.reason).toContain("no keys");
  });

  it("keeps an encrypting volume eligible — a partial key is still a key", () => {
    const [t] = buildTargets([], [bl({ volumeStatus: "EncryptionInProgress" })], "C:");
    expect(t.eligible).toBe(true);
  });
});

describe("deriveSystemDrive", () => {
  it("uses the OperatingSystem volume's mount point rather than assuming C:", () => {
    expect(deriveSystemDrive([bl({ mountPoint: "E:", volumeType: "OperatingSystem" })])).toBe("E:");
  });

  it("normalizes a trailing separator and case", () => {
    expect(deriveSystemDrive([bl({ mountPoint: "e:\\", volumeType: "OperatingSystem" })])).toBe("E:");
  });

  it("falls back to C: when no OS volume is reported", () => {
    expect(deriveSystemDrive([bl({ mountPoint: "D:" })])).toBe("C:");
    expect(deriveSystemDrive([])).toBe("C:");
  });
});

describe("expectedAckToken", () => {
  it("uses the BitLocker mount point, mirroring the server", () => {
    const [t] = buildTargets([], [bl({ mountPoint: "e:\\", volumeType: "OperatingSystem" })], "E:");
    expect(expectedAckToken(t, "E:")).toBe("E:");
  });

  it("uses the real system drive for a VeraCrypt raw-device target, not a hardcoded C:", () => {
    const [t] = buildTargets([{ letter: "\\\\.\\E:", path: "\\\\.\\PhysicalDrive0", type: "Mounted" }], [], "E:");
    expect(requiresNuclear(t)).toBe(true);
    expect(expectedAckToken(t, "E:")).toBe("E:");
  });
});

describe("targetSubject", () => {
  it("names the container path for VeraCrypt", () => {
    const [t] = buildTargets([{ letter: "V:", path: "D:\\vault.hc", type: "Mounted" }], [], "C:");
    expect(targetSubject(t)).toBe("D:\\vault.hc");
  });

  it("names the mount point for BitLocker", () => {
    const [t] = buildTargets([], [bl({ mountPoint: "D:" })], "C:");
    expect(targetSubject(t)).toBe("D:");
  });
});

describe("eraseMethodLabel", () => {
  it("distinguishes header overwrite from protector removal", () => {
    const [vc] = buildTargets([{ letter: "V:", path: "D:\\v.hc", type: "Mounted" }], [], "C:");
    const [b] = buildTargets([], [bl({})], "C:");
    expect(eraseMethodLabel(vc)).toContain("header");
    expect(eraseMethodLabel(b)).toContain("key protector");
  });
});

describe("eraseLimitation", () => {
  it("admits the VeraCrypt overwrite is not independently re-mounted", () => {
    const [t] = buildTargets([{ letter: "V:", path: "D:\\v.hc", type: "Mounted" }], [], "C:");
    expect(eraseLimitation(t)).toContain("does not re-mount");
  });

  it("states that an OS volume's key survives until power-off", () => {
    const [t] = buildTargets([], [bl({ mountPoint: "C:", volumeType: "OperatingSystem" })], "C:");
    expect(eraseLimitation(t)).toContain("powers off");
  });

  it("states the data-volume eviction caveat", () => {
    const [t] = buildTargets([], [bl({ mountPoint: "D:" })], "C:");
    expect(eraseLimitation(t)).toContain("stays readable");
  });
});

describe("eraseConsequences", () => {
  it("adds the won't-boot consequence only for an OS volume", () => {
    const [os] = buildTargets([], [bl({ mountPoint: "C:", volumeType: "OperatingSystem" })], "C:");
    const [data] = buildTargets([], [bl({ mountPoint: "D:" })], "C:");
    expect(eraseConsequences(os).some((c) => c.includes("will not start"))).toBe(true);
    expect(eraseConsequences(data).some((c) => c.includes("will not start"))).toBe(false);
  });

  it("adds the escrow revocation consequence when a recovery key is backed up", () => {
    const [t] = buildTargets([], [bl({ recoveryPasswordPresent: true })], "C:");
    expect(eraseConsequences(t).some((c) => c.includes("escrowed"))).toBe(true);
  });

  it("always states that the data becomes unreadable and a backup does not help", () => {
    const [t] = buildTargets([], [bl({})], "C:");
    expect(eraseConsequences(t).some((c) => c.includes("permanently unreadable"))).toBe(true);
    expect(eraseConsequences(t).some((c) => c.includes("does not help"))).toBe(true);
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
