// src/lib/stegoBackup.ts
//
// Pure domain model for Stego Backup (hide a VeraCrypt container inside a
// playable MP4). Extracted out of StegoBackupSection.tsx so the size/path/
// capacity rules can be tested without a Tauri host — the component used to
// decide everything inline and nothing here was reachable from a test.
//
// Technique (ARCHITECTURE.md:125): the container is APPENDED to the carrier
// with a `WCSTEGO1` + offset trailer. So the carrier is not a capacity
// ceiling — it is cover. What matters is (a) the output stays plausible as an
// ordinary video, and (b) the destination has room for carrier + container.

export type SizeUnit = "M" | "G" | "T";

export const BYTES_PER_MB = 1024 * 1024;

/** Whole megabytes are all the backend accepts, and FAT needs room for its own tables. */
export const MIN_CONTAINER_MB = 1;

/** The hidden volume is formatted FAT; FAT32 tops out at 2 TiB. */
export const MAX_CONTAINER_MB = 2 * 1024 * 1024;

export const MIN_PASSWORD_LENGTH = 8;

/**
 * A hidden volume this much smaller than its carrier keeps the output file's
 * size in the range a real video of that length would occupy. Below it the
 * payload dominates the file and the cover story stops working.
 */
export const PLAUSIBLE_CARRIER_RATIO = 3;

/** `WCSTEGO1` magic + a 64-bit payload offset (ARCHITECTURE.md:125). */
export const STEGO_TRAILER_BYTES = 16;

/**
 * `get_wipe_drive_list` reports free space rounded to 0.1 GB, so a reading can
 * understate the truth by half a step. Never hard-block inside that margin.
 */
export const FREE_SPACE_ROUNDING_BYTES = Math.round(0.05 * 1024 ** 3);

/** Windows tools still refuse paths past MAX_PATH unless long paths are enabled. */
export const MAX_PATH_LENGTH = 260;

/** The three containers the carrier file picker offers, i.e. what the handler is fed today. */
export const CARRIER_EXTENSIONS = ["mp4", "m4v", "mov"];

/** VeraCrypt only recognises these as container files when you go to mount one. */
export const CONTAINER_EXTENSIONS = ["hc", "tc"];

const UNIT_MB: Record<SizeUnit, number> = { M: 1, G: 1024, T: 1024 * 1024 };

/** Digits only — `Number()` would happily accept "1e9", "0x20" and " ". */
const PLAIN_DECIMAL = /^\d+(?:\.\d+)?$/;

export interface ContainerSizeParse {
  /** Whole megabytes, or null when the input cannot be used at all. */
  mb: number | null;
  error: string | null;
}

/**
 * Parse the size field. Never returns 0 silently: `useBackend.parseSizeToMB`
 * answers 0 for anything it cannot read, which is how a bare "20" used to
 * reach the handler as `SizeMB: 0`.
 */
export function parseContainerSize(raw: string, unit: SizeUnit): ContainerSizeParse {
  const trimmed = raw.trim();
  if (!trimmed) return { mb: null, error: "Enter how big the hidden volume should be." };
  if (!PLAIN_DECIMAL.test(trimmed)) {
    return { mb: null, error: `"${trimmed}" is not a plain number — type digits only, like 20.` };
  }
  const value = Number(trimmed);
  if (!Number.isFinite(value)) return { mb: null, error: `"${trimmed}" is not a number we can use.` };

  const mb = Math.round(value * UNIT_MB[unit]);
  if (mb < MIN_CONTAINER_MB) {
    return { mb: null, error: `The smallest hidden volume is ${MIN_CONTAINER_MB} MB.` };
  }
  if (mb > MAX_CONTAINER_MB) {
    return { mb: null, error: "The hidden volume is formatted FAT, so it cannot exceed 2048 GB." };
  }
  return { mb, error: null };
}

/**
 * Encode megabytes for `createStegoMp4({ size })`. The unit suffix is
 * mandatory: useBackend's parser matches /^(\d+(\.\d+)?)([MGT])$/ and returns
 * 0 for a bare number, so "20" would have created a 0 MB volume.
 */
export function toBackendSize(mb: number): string {
  return `${Math.round(mb)}M`;
}

/** Lower-case extension of the last path segment, or "" when there is none. */
export function fileExtension(path: string): string {
  const name = path.replace(/\//g, "\\").split("\\").pop() ?? "";
  const dot = name.lastIndexOf(".");
  if (dot <= 0 || dot === name.length - 1) return "";
  return name.slice(dot + 1).toLowerCase();
}

export function isSupportedCarrier(path: string): boolean {
  return CARRIER_EXTENSIONS.includes(fileExtension(path));
}

/** Give the recovered container a name VeraCrypt will offer to mount. */
export function normalizeContainerOutputPath(path: string): string {
  const trimmed = path.trim();
  if (!trimmed) return "";
  return CONTAINER_EXTENSIONS.includes(fileExtension(trimmed)) ? trimmed : `${trimmed}.hc`;
}

/** Windows-flavoured comparison: separators, duplicate separators and case all ignored. */
export function isSamePath(a: string, b: string): boolean {
  if (!a.trim() || !b.trim()) return false;
  return canonicalPath(a) === canonicalPath(b);
}

function canonicalPath(path: string): string {
  const slashed = path.trim().replace(/\//g, "\\");
  const isUnc = slashed.startsWith("\\\\");
  const collapsed = slashed.replace(/\\{2,}/g, "\\");
  const restored = isUnc ? `\\${collapsed}` : collapsed;
  return restored.replace(/\\+$/, "").toLowerCase();
}

/** Drive letter a path lives on, upper-cased. Null for UNC and relative paths. */
export function driveLetterOf(path: string): string | null {
  const match = path.trim().match(/^([A-Za-z]):[\\/]/);
  return match ? match[1].toUpperCase() : null;
}

export interface DriveFreeSpace {
  letter: string;
  freeGB: number;
}

/** Free bytes on the volume holding `path`, or null when we have no reading for it. */
export function freeBytesForPath(path: string, drives: DriveFreeSpace[]): number | null {
  const letter = driveLetterOf(path);
  if (!letter) return null;
  const drive = drives.find((d) => d.letter.toUpperCase() === letter);
  if (!drive || !Number.isFinite(drive.freeGB)) return null;
  return Math.round(drive.freeGB * 1024 ** 3);
}

/**
 * Bytes the destination must have free. With the carrier size unknown this is
 * a LOWER BOUND — the output also contains a full copy of the carrier.
 */
export function requiredFreeBytes(containerMb: number, carrierBytes: number | null): number {
  return containerMb * BYTES_PER_MB + STEGO_TRAILER_BYTES + (carrierBytes ?? 0);
}

export type CapacityVerdict = "unknown" | "ok" | "tight" | "implausible";

export interface CapacityPlan {
  containerBytes: number;
  /** Smallest carrier that keeps the output looking like an ordinary video. */
  minimumCarrierBytes: number;
  estimatedOutputBytes: number | null;
  /** Share of the output file that is payload, 0..1. */
  payloadShare: number | null;
  /** Carrier bytes above the plausible minimum; negative when short. */
  headroomBytes: number | null;
  verdict: CapacityVerdict;
}

/**
 * The capacity story shown before the user commits. `carrierBytes` is null
 * whenever the carrier cannot be measured — the webview has no file-system
 * permission for arbitrary paths — and then only the minimum-carrier
 * guidance is meaningful.
 */
export function describeCapacity(containerMb: number, carrierBytes: number | null): CapacityPlan {
  const containerBytes = containerMb * BYTES_PER_MB;
  const minimumCarrierBytes = containerBytes * PLAUSIBLE_CARRIER_RATIO;

  if (carrierBytes == null) {
    return {
      containerBytes,
      minimumCarrierBytes,
      estimatedOutputBytes: null,
      payloadShare: null,
      headroomBytes: null,
      verdict: "unknown",
    };
  }

  const estimatedOutputBytes = carrierBytes + containerBytes + STEGO_TRAILER_BYTES;
  return {
    containerBytes,
    minimumCarrierBytes,
    estimatedOutputBytes,
    payloadShare: containerBytes / estimatedOutputBytes,
    headroomBytes: carrierBytes - minimumCarrierBytes,
    verdict:
      carrierBytes >= minimumCarrierBytes ? "ok" : carrierBytes >= containerBytes ? "tight" : "implausible",
  };
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  const units = ["KB", "MB", "GB", "TB", "PB"];
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length);
  const scaled = bytes / 1024 ** exponent;
  return `${scaled.toFixed(scaled < 100 ? 1 : 0)} ${units[exponent - 1]}`;
}
