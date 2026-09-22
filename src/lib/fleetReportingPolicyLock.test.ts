import { describe, expect, test } from "bun:test";
import { isFleetReportControlLocked, isFleetReportingPathLocked } from "./fleetReportingPolicyLock";

describe("Fleet monitor-report policy locks", () => {
  test("recognizes exact, legacy ideal-prefixed, and ancestor locks", () => {
    expect(isFleetReportingPathLocked(
      ["privacy.screenCapture.reportToFleet"],
      "privacy.screenCapture.reportToFleet",
    )).toBe(true);
    expect(isFleetReportingPathLocked(
      ["ideal.privacy.screenCapture"],
      "privacy.screenCapture.reportToFleet",
    )).toBe(true);
  });

  test("does not lock a local reporter merely because a value is true", () => {
    expect(isFleetReportControlLocked({
      lockedPaths: [],
      reportPath: "privacy.decoyMonitor.fleetAlertEnabled",
      requireAllDeviceAlertsInFleet: true,
    })).toBe(false);
  });

  test("locks each local reporter when the signed Fleet master is active", () => {
    expect(isFleetReportControlLocked({
      lockedPaths: ["security.requireAllDeviceAlertsInFleet"],
      reportPath: "privacy.ransomwareMonitor.reportToFleet",
      requireAllDeviceAlertsInFleet: true,
    })).toBe(true);
  });
});
