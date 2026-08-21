export type PrivacyShieldMode = "blur_notify" | "notify_only";

export interface PrivacyShieldTriggers {
  gaze: boolean;
  faces: boolean;
  device: boolean;
}

export function resolvePrivacyShieldMode({
  fleetManaged,
  fleetMode,
  localMode,
}: {
  fleetManaged: boolean;
  fleetMode?: PrivacyShieldMode | null;
  localMode?: PrivacyShieldMode | null;
}): PrivacyShieldMode {
  if (fleetManaged && fleetMode) return fleetMode;
  return localMode === "notify_only" ? "notify_only" : "blur_notify";
}

export function privacyShieldBlurTriggers(
  mode: PrivacyShieldMode,
  triggers: PrivacyShieldTriggers,
): PrivacyShieldTriggers {
  return mode === "notify_only"
    ? { gaze: false, faces: false, device: false }
    : triggers;
}
