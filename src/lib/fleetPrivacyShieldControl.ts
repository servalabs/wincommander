import type { PrivacyShieldMode } from "./privacyShieldMode";

/**
 * Resolve the effective Fleet Privacy Shield instruction.
 *
 * The dedicated `shieldDesiredState` check-in field is newer than the signed
 * config-epoch flags.  It must win when present: otherwise a console request
 * can be saved and delivered successfully, but the device never starts the
 * Shield because it is still waiting for an unrelated config epoch.
 */
export function resolveFleetPrivacyShieldControl({
  fleetEnabled,
  legacyManaged,
  legacyMonitoringEnabled,
  desiredState,
}: {
  fleetEnabled: boolean;
  legacyManaged: boolean;
  legacyMonitoringEnabled: boolean;
  desiredState?: { enabled: boolean; mode: PrivacyShieldMode } | null;
}): { managed: boolean; enabled: boolean; mode?: PrivacyShieldMode } {
  if (!fleetEnabled) return { managed: false, enabled: false };

  // A resolved dedicated state is an explicit administrator instruction,
  // including `enabled: false`; do not fall back to stale legacy flags.
  if (desiredState) {
    return {
      managed: true,
      enabled: desiredState.enabled,
      mode: desiredState.mode,
    };
  }

  return {
    managed: legacyManaged,
    enabled: legacyManaged && legacyMonitoringEnabled,
  };
}
