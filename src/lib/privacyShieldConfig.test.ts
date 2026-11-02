import { describe, expect, test } from "bun:test";
import type { PrivacyShieldSettings } from "../types/settings";
import { defaultPrivacyShieldConfig, hydratePrivacyShieldConfig, privacyShieldConfigPatch } from "./privacyShieldConfig";

describe("Privacy Shield capture preferences", () => {
  test("capture and Fleet upload stay off for an unconfigured account", () => {
    const config = hydratePrivacyShieldConfig(defaultPrivacyShieldConfig(), {} as PrivacyShieldSettings);
    expect(config.captureOnDevice).toBe(false);
    expect(config.captureOnMultiFace).toBe(false);
    expect(config.sendIncidentCapturesToFleet).toBe(false);
  });

  test("opening a new card restores all saved capture settings rather than resetting them", () => {
    const selected = { ...defaultPrivacyShieldConfig(), captureOnDevice: true, captureOnMultiFace: true,
      sendIncidentCapturesToFleet: true, captureSpeed: 3, deviceWakeMultiplier: 7,
      multiFaceWakeMultiplier: 4, wakeDelayMs: 250 };
    const stored = privacyShieldConfigPatch(selected) as PrivacyShieldSettings;
    const reopened = hydratePrivacyShieldConfig(defaultPrivacyShieldConfig(), JSON.parse(JSON.stringify(stored)));
    expect(reopened).toEqual(selected);
  });

  test("disabling upload survives a restart without disabling local capture", () => {
    const config = { ...defaultPrivacyShieldConfig(), captureOnDevice: true, sendIncidentCapturesToFleet: false };
    const restored = hydratePrivacyShieldConfig(defaultPrivacyShieldConfig(), privacyShieldConfigPatch(config) as PrivacyShieldSettings);
    expect(restored.captureOnDevice).toBe(true);
    expect(restored.sendIncidentCapturesToFleet).toBe(false);
  });
});
