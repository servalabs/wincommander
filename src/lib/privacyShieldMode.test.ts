import { describe, expect, test } from "bun:test";
import {
  privacyShieldBlurTriggers,
  privacyShieldLookAwayDelay,
  privacyShieldDetectorGaze,
  fleetShieldDelayPolicyActive,
  fleetShieldDetectorPolicyActive,
  resolvePrivacyShieldMode,
} from "./privacyShieldMode";

describe("Privacy Shield alert modes", () => {
  const triggers = { gaze: true, faces: true, device: true };

  test("uses immediate local and ten-second Fleet defaults while preserving explicit zero", () => {
    expect(privacyShieldLookAwayDelay(null)).toBe(0);
    expect(privacyShieldLookAwayDelay({}, true)).toBe(10);
    expect(privacyShieldLookAwayDelay({ lookAwayDelaySeconds: 0 }, true, 5)).toBe(0);
    expect(privacyShieldLookAwayDelay({}, false, 23)).toBe(23);
    expect(privacyShieldLookAwayDelay({ lookAwayDelaySeconds: 30 })).toBe(0);
    expect(privacyShieldLookAwayDelay({ lookAwayDelaySeconds: 30 }, true, 5)).toBe(30);
    expect(privacyShieldLookAwayDelay({ lookAwayDelaySeconds: 30 }, false, 5)).toBe(5);
    expect(fleetShieldDelayPolicyActive(true, ["privacy.privacyShield"])).toBe(true);
    expect(fleetShieldDelayPolicyActive(true, ["ideal.privacy.privacyShield.lookAwayDelaySeconds"])).toBe(true);
    expect(fleetShieldDelayPolicyActive(true, ["privacy"])).toBe(true);
    expect(fleetShieldDelayPolicyActive(false, ["privacy.privacyShield"])).toBe(false);
    expect(fleetShieldDelayPolicyActive(true, [])).toBe(false);
    expect(fleetShieldDelayPolicyActive(true, ["privacy.privacyShield.blurOpacity"])).toBe(false);
    expect(fleetShieldDetectorPolicyActive(true, ["privacy.privacyShield.lookAwayDetectionEnabled"])).toBe(true);
    expect(fleetShieldDetectorPolicyActive(true, ["privacy.privacyShield.blurOpacity"])).toBe(false);
  });

  test("applies the Fleet detector switch on local starts without retaining it after detachment", () => {
    expect(privacyShieldDetectorGaze(true, { lookAwayDetectionEnabled: false }, true)).toBe(false);
    expect(privacyShieldDetectorGaze(false, {}, true)).toBe(true);
    expect(privacyShieldDetectorGaze(true, { lookAwayDetectionEnabled: false }, false)).toBe(true);
    expect(privacyShieldDetectorGaze(false, { lookAwayDetectionEnabled: true }, false)).toBe(false);
  });

  test("Notify Only keeps detectors active while suppressing every blur trigger", () => {
    expect(privacyShieldBlurTriggers("notify_only", triggers)).toEqual({
      gaze: false,
      faces: false,
      device: false,
    });
    expect(triggers).toEqual({ gaze: true, faces: true, device: true });
  });

  test("Blur + Notify preserves the selected blur triggers", () => {
    expect(privacyShieldBlurTriggers("blur_notify", triggers)).toEqual(triggers);
  });

  test("the signed Fleet mode takes precedence over the local preference", () => {
    expect(resolvePrivacyShieldMode({
      fleetManaged: true,
      fleetMode: "notify_only",
      localMode: "blur_notify",
    })).toBe("notify_only");
  });

  test("the local preference applies when Fleet does not supply a mode", () => {
    expect(resolvePrivacyShieldMode({
      fleetManaged: false,
      fleetMode: "notify_only",
      localMode: "notify_only",
    })).toBe("notify_only");
  });
});
