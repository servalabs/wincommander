import { expect, test } from "bun:test";
import { MIN_RAM_DISK_SIZE_MB, normalizeRamDiskSizeMB } from "./ramDisk";

test("RAM-disk sizes are never normalised below 256 MB", () => {
  expect(MIN_RAM_DISK_SIZE_MB).toBe(256);
  expect(normalizeRamDiskSizeMB(undefined)).toBe(256);
  expect(normalizeRamDiskSizeMB(1)).toBe(256);
  expect(normalizeRamDiskSizeMB(255.8)).toBe(256);
  expect(normalizeRamDiskSizeMB(256)).toBe(256);
  expect(normalizeRamDiskSizeMB(512.4)).toBe(512);
});
