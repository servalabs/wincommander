import { describe, expect, test } from "bun:test";
import { isLowSpecHardwareProfile, isWindows10, shouldReduceMotionForSystem } from "./motionPolicy";

describe("motionPolicy system defaults", () => {
  test("reduces motion on a Windows 10 system", () => {
    expect(isWindows10("Microsoft Windows 10 Pro")).toBe(true);
    expect(shouldReduceMotionForSystem({ osName: "Microsoft Windows 10 Pro", ramTotalGb: 16 })).toBe(true);
  });

  test("reduces motion below 8 GB RAM but not for a capable Windows 11 system", () => {
    expect(shouldReduceMotionForSystem({ osName: "Microsoft Windows 11 Pro", ramTotalGb: 7.9 })).toBe(true);
    expect(shouldReduceMotionForSystem({ osName: "Microsoft Windows 11 Pro", ramTotalGb: 8 })).toBe(false);
  });

  test("treats four logical cores as the low-spec boundary", () => {
    expect(isLowSpecHardwareProfile(4, 8)).toBe(true);
    expect(isLowSpecHardwareProfile(5, 8)).toBe(false);
  });
});
