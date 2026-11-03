export type PrivacyShieldMode = "blur_notify" | "notify_only";

export interface PrivacyShieldTriggers {
  gaze: boolean;
  faces: boolean;
  device: boolean;
}

/** A local unset preference is immediate; a Fleet-managed policy defaults to
 * ten seconds. An explicit zero remains a valid administrator choice. */
export function privacyShieldLookAwayDelay(
  settings: { lookAwayDelaySeconds?: number | null } | null | undefined,
  fleetManaged = false,
  localDelay?: number | null,
): number {
  return fleetManaged ? (settings?.lookAwayDelaySeconds ?? 10) : (localDelay ?? 0);
}

/** A signed Fleet policy controls detection even when the device starts Shield locally.
 * Without an active Fleet policy, never reuse a stale managed preference. */
export function privacyShieldDetectorGaze(local: boolean, settings: { lookAwayDetectionEnabled?: boolean | null } | null | undefined, fleetManaged: boolean): boolean {
  return fleetManaged ? (settings?.lookAwayDetectionEnabled ?? true) : local;
}

/** Only a lock on the field or its parent authorizes its Fleet value. */
function fleetShieldFieldPolicyActive(fleetEnabled: boolean, locks: readonly string[] | null | undefined, field: string): boolean {
  return fleetEnabled && !!locks?.some((path) => {
    const normalized = path.startsWith("ideal.") ? path.slice("ideal.".length) : path;
    return normalized === "" || normalized === "ideal" || normalized === "privacy" || normalized === "privacy.privacyShield" || normalized === `privacy.privacyShield.${field}`;
  });
}
export function fleetShieldDelayPolicyActive(fleetEnabled: boolean, locks: readonly string[] | null | undefined): boolean {
  return fleetShieldFieldPolicyActive(fleetEnabled, locks, "lookAwayDelaySeconds");
}
export function fleetShieldDetectorPolicyActive(fleetEnabled: boolean, locks: readonly string[] | null | undefined): boolean {
  return fleetShieldFieldPolicyActive(fleetEnabled, locks, "lookAwayDetectionEnabled");
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
