import { describe, expect, it } from "bun:test";
import {
  addVeracryptPath,
  isBitlockerDriveSelected,
  removeVeracryptPath,
  toggleBitlockerDrive,
} from "./cryptoEraseCascadeUtils";

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
