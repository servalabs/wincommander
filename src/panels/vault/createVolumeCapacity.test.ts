import { describe, expect, test } from "bun:test";
import { assessDestinationSpace, largeVaultNotice, sizeToBytes } from "./createVolumeCapacity";

const GIB = 1024 ** 3;

describe("Create Vault wizard size notices", () => {
  test("converts the wizard's size and unit to bytes and rejects nonsense", () => {
    expect(sizeToBytes(500, "M")).toBe(500 * 1024 ** 2);
    expect(sizeToBytes(1.5, "G")).toBe(1.5 * GIB);
    expect(sizeToBytes(2, "T")).toBe(2 * 1024 ** 4);
    for (const bad of [0, -3, Number.NaN, Number.POSITIVE_INFINITY]) expect(sizeToBytes(bad, "G")).toBe(0);
  });

  test("shows nothing below 2 GiB and the long-wait line from 2 GiB", () => {
    expect(largeVaultNotice(2 * GIB - 1, true)).toBeNull();
    expect(largeVaultNotice(2 * GIB, false)).toContain("several minutes");
    expect(largeVaultNotice(2 * GIB, false)).toContain("Keep WinCommander open");
  });

  test("a decoy Vault explains the random fill and a plain Vault does not", () => {
    expect(largeVaultNotice(50 * GIB, true)).toContain("random data");
    expect(largeVaultNotice(50 * GIB, false)).not.toContain("random data");
  });
});

describe("Create Vault wizard destination space check", () => {
  const base = { sizeBytes: 10 * GIB, isElevated: true, isDynamic: false };

  test("blocks a Vault that clearly exceeds the free space and states both figures", () => {
    const verdict = assessDestinationSpace({ ...base, freeBytes: 4 * GIB });
    expect(verdict.level).toBe("blocked");
    expect("message" in verdict && verdict.message).toContain("4.0 GB");
    expect("message" in verdict && verdict.message).toContain("10.0 GB");
  });

  test("never blocks inside the 0.1 GB rounding of the free-space reading", () => {
    expect(assessDestinationSpace({ ...base, freeBytes: 10 * GIB - 0.04 * GIB }).level).toBe("ok");
    expect(assessDestinationSpace({ ...base, freeBytes: 10 * GIB - 0.2 * GIB }).level).toBe("blocked");
  });

  test("warns about the 2x staging need only when not elevated", () => {
    const free = 15 * GIB;
    expect(assessDestinationSpace({ ...base, freeBytes: free, isElevated: false }).level).toBe("warning");
    expect(assessDestinationSpace({ ...base, freeBytes: free, isElevated: true }).level).toBe("ok");
    expect(assessDestinationSpace({ ...base, freeBytes: free, isElevated: null }).level).toBe("ok");
    expect(assessDestinationSpace({ ...base, freeBytes: 25 * GIB, isElevated: false }).level).toBe("ok");
  });

  test("an unreadable drive, an empty size or a dynamic container never block", () => {
    expect(assessDestinationSpace({ ...base, freeBytes: null }).level).toBe("ok");
    expect(assessDestinationSpace({ ...base, sizeBytes: 0, freeBytes: 0 }).level).toBe("ok");
    expect(assessDestinationSpace({ ...base, freeBytes: 1 * GIB, isDynamic: true }).level).toBe("ok");
  });
});
