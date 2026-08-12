// src/hooks/useManagedPolicy.test.ts
//
// Unit tests for the pure isToggleLocked() helper.
// No React / DOM / Tauri needed — these are pure function tests.

import { describe, expect, test } from "bun:test";
import { isToggleLocked, LOCK_MAP } from "./useManagedPolicy";

describe("isToggleLocked", () => {
  test("returns false when values is empty (no-policy default)", () => {
    expect(isToggleLocked({}, "telemetry")).toBe(false);
  });

  test("returns false when values object contains unrelated keys", () => {
    expect(isToggleLocked({ SomeOtherKey: true }, "telemetry")).toBe(false);
  });

  test("returns true when LockTelemetryOff is truthy and querying telemetry toggle", () => {
    expect(isToggleLocked({ LockTelemetryOff: true }, "telemetry")).toBe(true);
  });

  test("returns false when LockTelemetryOff is false (policy key present but not set)", () => {
    expect(isToggleLocked({ LockTelemetryOff: false }, "telemetry")).toBe(false);
  });

  test("returns false when LockTelemetryOff is truthy but querying a different toggle", () => {
    expect(isToggleLocked({ LockTelemetryOff: true }, "clipboardHistory")).toBe(false);
  });

  test("LOCK_MAP maps LockTelemetryOff to the telemetry toggle id", () => {
    expect(LOCK_MAP["LockTelemetryOff"]).toBe("telemetry");
  });

  test("toggles not in LOCK_MAP are never locked regardless of values", () => {
    // Even if a caller passes arbitrary policy keys, unlisted toggles stay unlocked.
    const arbitraryValues = { LockTelemetryOff: true, ForceSecureDNS: "1.1.1.1" };
    expect(isToggleLocked(arbitraryValues, "recentFiles")).toBe(false);
    expect(isToggleLocked(arbitraryValues, "activity")).toBe(false);
  });
});
