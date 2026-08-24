import { describe, expect, test } from "bun:test";
import { resolveFleetPrivacyShieldControl } from "./fleetPrivacyShieldControl";

describe("resolveFleetPrivacyShieldControl", () => {
  test("starts from the dedicated desired state without waiting for a config epoch", () => {
    expect(resolveFleetPrivacyShieldControl({
      fleetEnabled: true,
      legacyManaged: false,
      legacyMonitoringEnabled: false,
      desiredState: { enabled: true, mode: "notify_only" },
    })).toEqual({ managed: true, enabled: true, mode: "notify_only" });
  });

  test("an explicit Fleet stop overrides stale legacy enable flags", () => {
    expect(resolveFleetPrivacyShieldControl({
      fleetEnabled: true,
      legacyManaged: true,
      legacyMonitoringEnabled: true,
      desiredState: { enabled: false, mode: "blur_notify" },
    })).toEqual({ managed: true, enabled: false, mode: "blur_notify" });
  });

  test("keeps compatibility with servers that only publish the legacy policy", () => {
    expect(resolveFleetPrivacyShieldControl({
      fleetEnabled: true,
      legacyManaged: true,
      legacyMonitoringEnabled: true,
    })).toEqual({ managed: true, enabled: true });
  });
});
