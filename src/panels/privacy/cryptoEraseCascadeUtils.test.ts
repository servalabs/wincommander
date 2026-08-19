import { describe, expect, it } from "bun:test";
import {
  addVeracryptPath,
  isBitlockerDriveSelected,
  removeVeracryptDevice,
  removeVeracryptPath,
  toggleBitlockerDrive,
  veracryptDeviceIdentity,
} from "./cryptoEraseCascadeUtils";
import type { VeraCryptDeviceEraseTarget } from "../../types/settings";

const enrolledDevice = (
  overrides: Partial<VeraCryptDeviceEraseTarget> = {},
): VeraCryptDeviceEraseTarget => ({
  devicePath: "\\Device\\Harddisk1\\Partition6",
  diskNumber: 1,
  partitionNumber: 6,
  partitionGuid: "{AABBCCDD-0000-1111-2222-333344445555}",
  offsetBytes: 1_048_576,
  sizeBytes: 5_368_709_120,
  diskUniqueId: "{DISK-IDENTITY}",
  label: "PRIVATE",
  ...overrides,
});

describe("toggleBitlockerDrive", () => {
  // Settings patches REPLACE arrays wholesale, so every return value must be
  // the full desired array — a delta would silently drop the other selections.
  it("returns the full array with the drive added", () => {
    expect(toggleBitlockerDrive(["D:"], "E:")).toEqual(["D:", "E:"]);
  });

  it("removes a drive that is already selected", () => {
    expect(toggleBitlockerDrive(["D:", "E:"], "D:")).toEqual(["E:"]);
  });

  it("normalizes case and a trailing separator before comparing", () => {
    expect(toggleBitlockerDrive(["D:"], "d:\\")).toEqual([]);
  });

  it("stores the normalized form when adding", () => {
    expect(toggleBitlockerDrive([], "e:\\")).toEqual(["E:"]);
  });

  it("does not mutate the input array", () => {
    const current = ["D:"];
    toggleBitlockerDrive(current, "E:");
    expect(current).toEqual(["D:"]);
  });
});

describe("isBitlockerDriveSelected", () => {
  it("matches regardless of case or trailing separator", () => {
    expect(isBitlockerDriveSelected(["d:"], "D:\\")).toBe(true);
  });

  it("is false for an unselected drive", () => {
    expect(isBitlockerDriveSelected(["D:"], "E:")).toBe(false);
  });
});

describe("addVeracryptPath", () => {
  it("appends a new path", () => {
    expect(addVeracryptPath(["a.hc"], "b.hc")).toEqual(["a.hc", "b.hc"]);
  });

  it("returns the identical array when the path is already listed, so callers can detect a no-op", () => {
    const current = ["a.hc"];
    expect(addVeracryptPath(current, "a.hc")).toBe(current);
  });

  it("ignores a blank path", () => {
    const current = ["a.hc"];
    expect(addVeracryptPath(current, "   ")).toBe(current);
  });

  it("keeps two paths that differ only in case — filesystem paths can be case-sensitive", () => {
    expect(addVeracryptPath(["A.hc"], "a.hc")).toEqual(["A.hc", "a.hc"]);
  });
});

describe("removeVeracryptPath", () => {
  it("drops only the matching path", () => {
    expect(removeVeracryptPath(["a.hc", "b.hc"], "a.hc")).toEqual(["b.hc"]);
  });

  it("is a no-op for an unknown path", () => {
    expect(removeVeracryptPath(["a.hc"], "z.hc")).toEqual(["a.hc"]);
  });
});

describe("veracryptDeviceIdentity", () => {
  it("uses stable disk and partition identity instead of a reassignable device path", () => {
    const first = enrolledDevice();
    const readdressed = enrolledDevice({
      devicePath: "\\Device\\Harddisk4\\Partition2",
      diskNumber: 4,
      partitionNumber: 2,
    });

    expect(veracryptDeviceIdentity(first)).toBe(veracryptDeviceIdentity(readdressed));
  });

  it("normalizes GUID braces and case", () => {
    const first = enrolledDevice();
    const normalized = enrolledDevice({
      partitionGuid: "aabbccdd-0000-1111-2222-333344445555",
      diskUniqueId: "disk-identity",
    });

    expect(veracryptDeviceIdentity(first)).toBe(veracryptDeviceIdentity(normalized));
  });

  it("fails closed to the stored device path when stable identity is incomplete", () => {
    expect(veracryptDeviceIdentity(enrolledDevice({ partitionGuid: "" }))).toBe(
      "\\device\\harddisk1\\partition6",
    );
  });
});

describe("removeVeracryptDevice", () => {
  it("removes only the explicitly enrolled target with the same stable identity", () => {
    const target = enrolledDevice();
    const other = enrolledDevice({
      devicePath: "\\Device\\Harddisk1\\Partition7",
      partitionNumber: 7,
      partitionGuid: "{BBBBCCCC-0000-1111-2222-333344445555}",
      offsetBytes: 5_369_757_696,
    });

    expect(removeVeracryptDevice([target, other], target)).toEqual([other]);
  });

  it("does not mutate persisted targets", () => {
    const target = enrolledDevice();
    const current = [target];

    removeVeracryptDevice(current, target);

    expect(current).toEqual([target]);
  });
});
