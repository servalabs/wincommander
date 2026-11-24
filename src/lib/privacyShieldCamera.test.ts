import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import {
  CAMERA_UNCONFIRMED_MESSAGE,
  NO_USABLE_CAMERA_MESSAGE,
  cameraViewFromPresence,
  shieldCameraView,
} from "./privacyShieldCamera";

describe("Privacy Shield camera state", () => {
  test("a camera-less machine parses to the definite no-camera state, whatever the session", () => {
    // Shape returned by Get-PrivacyShieldStatus on an RDP Windows Server with no camera.
    const status = { running: false, cameraAvailable: false, cameraStatus: "hardware_unavailable", cameraPresence: "absent", cameraMessage: "No usable camera detected.", isWindowsServer: true };
    expect(shieldCameraView(status)).toEqual({ available: false, message: NO_USABLE_CAMERA_MESSAGE });
    expect(shieldCameraView({ cameraStatus: "hardware_unavailable", cameraAvailable: false })).toEqual({ available: false, message: NO_USABLE_CAMERA_MESSAGE });
  });

  test("a present camera shows no warning", () => {
    expect(shieldCameraView({ cameraAvailable: true, cameraStatus: "available", cameraPresence: "present", cameraMessage: "Camera available." }))
      .toEqual({ available: true, message: "Camera available." });
    expect(shieldCameraView({ cameraAvailable: true, cameraStatus: "available" }).message).toBeNull();
  });

  test("a probe failure is one calm sentence and does not claim the camera is missing", () => {
    for (const status of [{ cameraStatus: "unknown", cameraAvailable: false }, { cameraAvailable: null }, {}]) {
      expect(shieldCameraView(status)).toEqual({ available: null, message: CAMERA_UNCONFIRMED_MESSAGE });
    }
    expect(/could not be confirmed|unable to verify/i.test(CAMERA_UNCONFIRMED_MESSAGE)).toBe(false);
    expect(shieldCameraView({ cameraStatus: "unknown", cameraMessage: "  " }).message).toBe(CAMERA_UNCONFIRMED_MESSAGE);
    expect(shieldCameraView({ cameraStatus: "unknown", cameraPresence: "host_absent", cameraMessage: "Use the refresh icon." }))
      .toEqual({ available: null, message: "Use the refresh icon." });
  });

  test("a policy block keeps its explanation", () => {
    const message = "Camera is blocked by Windows policy: The device Camera policy disables camera access.";
    expect(shieldCameraView({ cameraAvailable: false, cameraStatus: "windows_camera_policy_denied", cameraPresence: "present", cameraMessage: message }))
      .toEqual({ available: false, message });
  });

  test("published presence maps to the card's initial and live state", () => {
    expect(cameraViewFromPresence("absent")).toEqual({ available: false, message: NO_USABLE_CAMERA_MESSAGE });
    expect(cameraViewFromPresence("host_absent")?.available).toBeNull();
    expect(cameraViewFromPresence("host_absent")?.message).toContain("refresh icon");
    expect(cameraViewFromPresence("present")).toBeNull();
    expect(cameraViewFromPresence("unknown")).toBeNull();
    expect(cameraViewFromPresence(null)).toBeNull();
  });

  test("the card has no unconditional unconfirmed-camera warning", () => {
    const card = readFileSync("src/panels/privacy/PrivacyShieldCard.tsx", "utf8");
    expect(card).not.toContain("Camera availability could not be confirmed");
    expect(card).not.toContain("Camera status could not be confirmed");
  });
});
