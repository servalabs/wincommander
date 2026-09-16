import { describe, expect, test } from "bun:test";
import {
  resolveFleetPrivacyShieldControl,
  resolveLocalFleetPrivacyShieldControl,
} from "./fleetPrivacyShieldControl";

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

  test("locks local Stop only for a Fleet-owned running session", () => {
    expect(resolveLocalFleetPrivacyShieldControl({
      running: true,
      sessionOwned: true,
      fleetControl: { managed: true, enabled: true },
    })).toEqual({ stopLocked: true, startLocked: false, settingsLocked: true });

    expect(resolveLocalFleetPrivacyShieldControl({
      running: true,
      sessionOwned: false,
      fleetControl: { managed: true, enabled: true },
    })).toEqual({ stopLocked: false, startLocked: false, settingsLocked: false });
  });

  test("restores local controls after Fleet's stopped read-back", () => {
    expect(resolveLocalFleetPrivacyShieldControl({
      running: false,
      // A delayed settings write cannot leave the card locked after an
      // independent process probe has verified that Shield is stopped.
      sessionOwned: true,
      fleetControl: { managed: true, enabled: false },
    })).toEqual({ stopLocked: false, startLocked: false, settingsLocked: false });
  });
});
