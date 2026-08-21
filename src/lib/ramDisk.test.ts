import { expect, test } from "bun:test";
import { MIN_RAM_DISK_SIZE_MB, normalizeRamDiskSizeMB, savedRamDiskMountRequest } from "./ramDisk";

test("RAM-disk sizes are never normalised below 256 MB", () => {
  expect(MIN_RAM_DISK_SIZE_MB).toBe(256);
  expect(normalizeRamDiskSizeMB(undefined)).toBe(256);
  expect(normalizeRamDiskSizeMB(1)).toBe(256);
  expect(normalizeRamDiskSizeMB(255.8)).toBe(256);
  expect(normalizeRamDiskSizeMB(256)).toBe(256);
  expect(normalizeRamDiskSizeMB(512.4)).toBe(512);
});

test("saved RAM-disk sizes carry unchanged into the shared mount request", () => {
  for (const sizeMB of [256, 512, 1024, 4096, 13312]) {
    expect(savedRamDiskMountRequest({
      enabled: true,
      sizeMB,
      driveLetter: "t",
      filesystem: "exFAT",
      label: "SCRATCH",
      readOnly: true,
    })).toEqual({
      SizeMB: sizeMB,
      DriveLetter: "T",
      Filesystem: "exFAT",
      Label: "SCRATCH",
      ReadOnly: true,
      Quick: true,
    });
  }
});

test("a missing or undersized saved RAM-disk size cannot create a fallback disk", () => {
  expect(savedRamDiskMountRequest({ enabled: true })).toBeNull();
  expect(savedRamDiskMountRequest({ enabled: true, sizeMB: 255 })).toBeNull();
});
