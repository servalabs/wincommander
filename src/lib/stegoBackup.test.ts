import { describe, it, expect } from "bun:test";
import {
  BYTES_PER_MB,
  MAX_CONTAINER_MB,
  describeCapacity,
  driveLetterOf,
  fileExtension,
  formatBytes,
  freeBytesForPath,
  isSamePath,
  isSupportedCarrier,
  normalizeContainerOutputPath,
  parseContainerSize,
  requiredFreeBytes,
  toBackendSize,
  type SizeUnit,
} from "./stegoBackup";

// Mirrors useBackend.parseSizeToMB (src/hooks/useBackend.ts:642-661), the
// consumer of toBackendSize(). Copied on purpose: these tests pin the contract
// between the two, so if that parser changes this mirror must change with it.
const parseSizeAsUseBackendDoes = (size: string): number => {
  const raw = size.trim().toUpperCase();
  const match = raw.match(/^(\d+(?:\.\d+)?)([MGT])$/);
  if (!match) return 0;
  const value = Number(match[1]);
  if (!Number.isFinite(value) || value <= 0) return 0;
  if (match[2] === "M") return Math.round(value);
  if (match[2] === "G") return Math.round(value * 1024);
  return Math.round(value * 1024 * 1024);
};

describe("parseContainerSize", () => {
  it("reads a plain number in each unit", () => {
    expect(parseContainerSize("20", "M").mb).toBe(20);
    expect(parseContainerSize("20", "G").mb).toBe(20480);
    expect(parseContainerSize("1", "T").mb).toBe(1048576);
  });

  it("tolerates surrounding whitespace from a paste", () => {
    expect(parseContainerSize("  20  ", "M").mb).toBe(20);
  });

  it("rejects a zero-byte payload instead of asking the engine to format nothing", () => {
    const parsed = parseContainerSize("0", "M");
    expect(parsed.mb).toBeNull();
    expect(parsed.error).toContain("smallest");
  });

  it("rejects a fraction that would round down to zero megabytes", () => {
    expect(parseContainerSize("0.4", "M").mb).toBeNull();
  });

  it("rounds a half megabyte up rather than truncating to zero", () => {
    expect(parseContainerSize("0.5", "M").mb).toBe(1);
    expect(parseContainerSize("20.5", "M").mb).toBe(21);
  });

  it("rejects a negative size", () => {
    const parsed = parseContainerSize("-5", "M");
    expect(parsed.mb).toBeNull();
    expect(parsed.error).toContain("plain number");
  });

  it("rejects an empty or whitespace-only size", () => {
    expect(parseContainerSize("", "M").error).toContain("Enter how big");
    expect(parseContainerSize("   ", "M").error).toContain("Enter how big");
  });

  it("rejects text, exponent notation, hex, doubled decimal points and non-ASCII digits", () => {
    for (const raw of ["abc", "1e9", "0x20", "20.5.1", "20 MB", "twenty", "٢٠"]) {
      expect(parseContainerSize(raw, "M").mb).toBeNull();
    }
  });

  it("accepts the FAT ceiling exactly and refuses one step past it", () => {
    expect(parseContainerSize("2", "T").mb).toBe(MAX_CONTAINER_MB);
    const tooBig = parseContainerSize("2.1", "T");
    expect(tooBig.mb).toBeNull();
    expect(tooBig.error).toContain("2048 GB");
  });

  it("rejects a very large payload rather than overflowing silently", () => {
    expect(parseContainerSize(String(Number.MAX_SAFE_INTEGER), "T").mb).toBeNull();
  });
});

describe("toBackendSize", () => {
  it("a bare number reaches the backend parser as 0 MB — the unit suffix is what stops that", () => {
    expect(parseSizeAsUseBackendDoes("20")).toBe(0);
    expect(parseSizeAsUseBackendDoes(toBackendSize(20))).toBe(20);
  });

  it("keeps the size unchanged across the client-to-backend encoding for every unit", () => {
    const cases: [string, SizeUnit, number][] = [["20", "M", 20], ["0.5", "G", 512], ["20", "G", 20480], ["1", "T", 1048576]];
    for (const [raw, unit, mb] of cases) {
      expect(parseContainerSize(raw, unit).mb).toBe(mb);
      expect(parseSizeAsUseBackendDoes(toBackendSize(mb))).toBe(mb);
    }
  });
});

describe("isSupportedCarrier", () => {
  it("accepts the three containers the file picker offers, whatever the case", () => {
    for (const path of ["C:\\v\\clip.mp4", "C:\\v\\CLIP.MP4", "C:\\v\\clip.m4v", "C:\\v\\clip.MOV"]) {
      expect(isSupportedCarrier(path)).toBe(true);
    }
  });

  it("rejects video containers the handler was never given", () => {
    for (const path of ["clip.mkv", "clip.avi", "clip.webm", "clip.wmv"]) {
      expect(isSupportedCarrier(path)).toBe(false);
    }
  });

  it("rejects a double extension that only looks like a video, and a file with none", () => {
    expect(isSupportedCarrier("C:\\v\\clip.mp4.exe")).toBe(false);
    expect(isSupportedCarrier("C:\\v\\clip")).toBe(false);
  });

  it("accepts paths with spaces, unicode and UNC roots unchanged", () => {
    expect(isSupportedCarrier("C:\\My Videos\\holiday clip.mp4")).toBe(true);
    expect(isSupportedCarrier("C:\\Users\\Ольга\\Видео\\клип.mp4")).toBe(true);
    expect(isSupportedCarrier("D:\\映画\\休暇 🎬.mp4")).toBe(true);
    expect(isSupportedCarrier("\\\\NAS\\media\\clip.mp4")).toBe(true);
  });

  it("does not read an extension out of a folder name", () => {
    expect(fileExtension("C:\\v1.2\\clip")).toBe("");
    expect(isSupportedCarrier("C:\\v1.2\\clip")).toBe(false);
  });
});

describe("normalizeContainerOutputPath", () => {
  it("appends .hc so VeraCrypt will offer to mount the recovered file", () => {
    expect(normalizeContainerOutputPath("C:\\out\\recovered")).toBe("C:\\out\\recovered.hc");
  });

  it("keeps an existing container extension, including its case", () => {
    expect(normalizeContainerOutputPath("C:\\out\\recovered.hc")).toBe("C:\\out\\recovered.hc");
    expect(normalizeContainerOutputPath("C:\\out\\recovered.HC")).toBe("C:\\out\\recovered.HC");
    expect(normalizeContainerOutputPath("C:\\out\\recovered.tc")).toBe("C:\\out\\recovered.tc");
  });

  it("appends rather than replaces when the name already has an unrelated extension", () => {
    expect(normalizeContainerOutputPath("C:\\out\\vault.backup")).toBe("C:\\out\\vault.backup.hc");
  });

  it("leaves spaces and unicode in the name untouched", () => {
    expect(normalizeContainerOutputPath("C:\\My Files\\the backup")).toBe("C:\\My Files\\the backup.hc");
    expect(normalizeContainerOutputPath("D:\\Δίσκος\\αντίγραφο")).toBe("D:\\Δίσκος\\αντίγραφο.hc");
  });

  it("is idempotent, so re-validating a form never stacks .hc.hc", () => {
    const once = normalizeContainerOutputPath("C:\\out\\recovered");
    expect(normalizeContainerOutputPath(once)).toBe(once);
  });

  it("returns empty for an empty path instead of a bare .hc", () => {
    expect(normalizeContainerOutputPath("")).toBe("");
    expect(normalizeContainerOutputPath("   ")).toBe("");
  });
});

describe("isSamePath", () => {
  it("catches the same file written with different separators, case or doubled separators", () => {
    expect(isSamePath("C:\\Videos\\clip.mp4", "c:/videos/clip.mp4")).toBe(true);
    expect(isSamePath("C:\\a\\\\b.mp4", "C:\\a\\b.mp4")).toBe(true);
  });

  it("matches a UNC path written with forward slashes", () => {
    expect(isSamePath("\\\\NAS\\m\\c.mp4", "//NAS/m/c.mp4")).toBe(true);
    expect(isSamePath("\\\\NAS\\m\\c.mp4", "\\\\OTHER\\m\\c.mp4")).toBe(false);
  });

  it("a share on a host is not the same file as a root-relative path of the same name", () => {
    // Collapsing runs of separators must not turn \\NAS\… into \NAS\…, or a
    // legitimate output path gets rejected as "you are overwriting the carrier".
    expect(isSamePath("\\\\NAS\\m\\c.mp4", "\\NAS\\m\\c.mp4")).toBe(false);
  });

  it("ignores a trailing separator but not a different file name", () => {
    expect(isSamePath("C:\\a\\", "C:\\a")).toBe(true);
    expect(isSamePath("C:\\a\\clip.mp4", "C:\\a\\other.mp4")).toBe(false);
  });

  it("two blank paths are not 'the same file', so the required-field error wins", () => {
    expect(isSamePath("", "")).toBe(false);
    expect(isSamePath("C:\\a\\clip.mp4", "  ")).toBe(false);
  });
});

describe("freeBytesForPath", () => {
  const drives = [
    { letter: "C", freeGB: 40.5 },
    { letter: "d", freeGB: 12.4 },
  ];

  it("matches the destination drive regardless of letter case", () => {
    expect(freeBytesForPath("D:\\out\\backup.mp4", drives)).toBe(Math.round(12.4 * 1024 ** 3));
    expect(freeBytesForPath("c:/out/backup.mp4", drives)).toBe(Math.round(40.5 * 1024 ** 3));
  });

  it("returns null for a drive with no reading, so nothing is blocked on a guess", () => {
    expect(freeBytesForPath("E:\\out\\backup.mp4", drives)).toBeNull();
  });

  it("returns null for a UNC or relative destination it cannot attribute to a drive", () => {
    expect(freeBytesForPath("\\\\NAS\\share\\backup.mp4", drives)).toBeNull();
    expect(freeBytesForPath("backup.mp4", drives)).toBeNull();
    expect(driveLetterOf("\\\\NAS\\share\\backup.mp4")).toBeNull();
  });
});

describe("requiredFreeBytes", () => {
  it("counts the trailer as well as the volume", () => {
    expect(requiredFreeBytes(20, null)).toBe(20 * BYTES_PER_MB + 16);
  });

  it("adds the carrier once its size is known, because the output contains a whole copy", () => {
    expect(requiredFreeBytes(20, 5_000_000)).toBe(20 * BYTES_PER_MB + 16 + 5_000_000);
  });
});

describe("describeCapacity", () => {
  const TEN_MB = 10 * BYTES_PER_MB;

  it("reports 'unknown' — not a false all-clear — when the carrier cannot be measured", () => {
    const plan = describeCapacity(10, null);
    expect(plan.verdict).toBe("unknown");
    expect(plan.estimatedOutputBytes).toBeNull();
    expect(plan.payloadShare).toBeNull();
    expect(plan.headroomBytes).toBeNull();
    expect(plan.minimumCarrierBytes).toBe(TEN_MB * 3);
  });

  it("a carrier exactly at the plausible minimum is ok with zero headroom", () => {
    const plan = describeCapacity(10, TEN_MB * 3);
    expect(plan.verdict).toBe("ok");
    expect(plan.headroomBytes).toBe(0);
  });

  it("one byte below the plausible minimum is tight, not ok", () => {
    const plan = describeCapacity(10, TEN_MB * 3 - 1);
    expect(plan.verdict).toBe("tight");
    expect(plan.headroomBytes).toBe(-1);
  });

  it("a payload exactly the size of its carrier is tight", () => {
    expect(describeCapacity(10, TEN_MB).verdict).toBe("tight");
  });

  it("a payload one byte larger than its carrier is implausible", () => {
    expect(describeCapacity(10, TEN_MB - 1).verdict).toBe("implausible");
  });

  it("a payload far larger than its carrier is implausible", () => {
    expect(describeCapacity(500, TEN_MB).verdict).toBe("implausible");
  });

  it("an empty carrier has no usable cover, so any payload is implausible", () => {
    const plan = describeCapacity(10, 0);
    expect(plan.verdict).toBe("implausible");
    expect(plan.estimatedOutputBytes).toBe(TEN_MB + 16);
  });

  it("estimates the output as carrier plus volume plus trailer", () => {
    const plan = describeCapacity(10, 100 * BYTES_PER_MB);
    expect(plan.estimatedOutputBytes).toBe(100 * BYTES_PER_MB + TEN_MB + 16);
    expect(plan.verdict).toBe("ok");
  });

  it("payload share shrinks as the carrier grows", () => {
    const share = (carrierMb: number) => describeCapacity(10, carrierMb * BYTES_PER_MB).payloadShare ?? 1;
    expect(share(30) > share(300)).toBe(true);
    expect(share(300) > 0).toBe(true);
  });

  it("stays inside safe-integer range at the FAT ceiling", () => {
    const plan = describeCapacity(MAX_CONTAINER_MB, 4 * BYTES_PER_MB);
    expect(Number.isSafeInteger(plan.minimumCarrierBytes)).toBe(true);
    expect(Number.isSafeInteger(plan.estimatedOutputBytes ?? 0)).toBe(true);
    expect(plan.verdict).toBe("implausible");
  });

  it("a bigger volume always needs a bigger carrier", () => {
    expect(describeCapacity(20, null).minimumCarrierBytes > describeCapacity(10, null).minimumCarrierBytes).toBe(true);
  });
});

describe("formatBytes", () => {
  it("labels sizes the way the capacity readout shows them", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1024)).toBe("1.0 KB");
    expect(formatBytes(20 * BYTES_PER_MB)).toBe("20.0 MB");
    expect(formatBytes(150 * BYTES_PER_MB)).toBe("150 MB");
    expect(formatBytes(1.5 * 1024 ** 3)).toBe("1.5 GB");
  });
});
