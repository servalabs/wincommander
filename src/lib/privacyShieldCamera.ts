import type { CameraPresence } from "../hooks/cameraPresenceProbe";

export interface ShieldCameraView {
  /** false only for a definite no-camera or blocked answer; null means not confirmed. */
  available: boolean | null;
  message: string | null;
}

export const NO_USABLE_CAMERA_MESSAGE = "No usable camera detected.";
export const CAMERA_UNCONFIRMED_MESSAGE = "Windows did not answer the camera device query. Use the refresh icon to check again.";
const VIDEO_SOURCES_UNLISTED_MESSAGE = "No camera device found, but Windows did not list its video sources. Use the refresh icon to check again.";

/** The view a published presence implies; null when presence says nothing about a missing camera. */
export function cameraViewFromPresence(presence: CameraPresence | null): ShieldCameraView | null {
  if (presence === "absent") return { available: false, message: NO_USABLE_CAMERA_MESSAGE };
  if (presence === "host_absent") return { available: null, message: VIDEO_SOURCES_UNLISTED_MESSAGE };
  return null;
}

/** Maps a Get-PrivacyShieldStatus camera probe result to what the card shows. */
export function shieldCameraView(data: {
  cameraAvailable?: unknown;
  cameraStatus?: unknown;
  cameraPresence?: unknown;
  cameraMessage?: unknown;
}): ShieldCameraView {
  const message = typeof data.cameraMessage === "string" && data.cameraMessage.trim() ? data.cameraMessage : null;
  if (data.cameraPresence === "absent" || data.cameraStatus === "hardware_unavailable") {
    return { available: false, message: NO_USABLE_CAMERA_MESSAGE };
  }
  if (data.cameraStatus === "unknown" || typeof data.cameraAvailable !== "boolean") {
    return { available: null, message: message ?? CAMERA_UNCONFIRMED_MESSAGE };
  }
  return { available: data.cameraAvailable, message };
}
