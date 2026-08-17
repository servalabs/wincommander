import { describe, expect, test } from "bun:test";
import {
  privacyShieldBlurTriggers,
  resolvePrivacyShieldMode,
} from "./privacyShieldMode";

describe("Privacy Shield alert modes", () => {
  const triggers = { gaze: true, faces: true, device: true };

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
