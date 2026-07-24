import type { DependencyInfo } from "../hooks/useDependencies";
import type { AppSettings, ExperienceLevel } from "../types/settings";
import type { ModuleConfig } from "../types/modules";
import type { CapabilityBundle, Density } from "../types/persona";

function mapExperienceLevelToDensity(level: ExperienceLevel | undefined): Density {
  return level === "advanced" ? "expert" : "guided";
}

function pushCapability(
  target: Set<CapabilityBundle>,
  enabled: boolean,
  capability: CapabilityBundle,
): void {
  if (enabled) {
    target.add(capability);
  }
}

export function getDensityForSettings(appSettings: AppSettings | null | undefined): Density {
  return appSettings?.app?.density ?? mapExperienceLevelToDensity(appSettings?.app?.experienceLevel);
}

export function getCapabilitiesForSettings(
  appSettings: AppSettings | null | undefined,
): CapabilityBundle[] {
  const persisted = appSettings?.app?.capabilities;
  if (persisted && persisted.length > 0) {
    return Array.from(new Set(persisted));
  }

  const modules = appSettings?.app?.modules ?? {};
  const capabilities = new Set<CapabilityBundle>();

  pushCapability(capabilities, modules.privacy === true, "privacy");
  pushCapability(capabilities, modules.network === true || modules.mesh === true, "network");
  pushCapability(
    capabilities,
    appSettings?.app?.privacyCleanEnabled === true,
    "safeguards",
  );
  pushCapability(
    capabilities,
    appSettings?.current?.privacy?.clipboard?.pasteMonitorEnabled === true ||
      appSettings?.current?.privacy?.decoyMonitor?.enabled === true ||
      appSettings?.current?.privacy?.ransomwareMonitor?.enabled === true ||
      appSettings?.current?.privacy?.remoteAccessMonitor?.enabled === true ||
      appSettings?.current?.privacy?.screenCapture?.detectionEnabled === true ||
      appSettings?.current?.privacy?.screenCapture?.protectWindow === true ||
      appSettings?.current?.privacy?.privacyShield?.gazeDetectionEnabled === true ||
      appSettings?.current?.privacy?.privacyShield?.antiPeepingEnabled === true ||
      appSettings?.current?.privacy?.privacyShield?.cameraHunterEnabled === true,
    "monitoring",
  );

  return Array.from(capabilities);
}

export function getPersonaForSetupChoices(
  experienceLevel: ExperienceLevel,
  privacyCleanEnabled: boolean,
  modules: ModuleConfig,
): { density: Density; capabilities: CapabilityBundle[] } {
  // KT: this mirrors getCapabilitiesForSettings so setup choices and
  // legacy-settings migration bucket users the same way during U0-U4.
  const capabilities = new Set<CapabilityBundle>();

  pushCapability(capabilities, modules.privacy === true, "privacy");
  pushCapability(capabilities, modules.network === true || modules.mesh === true, "network");
  pushCapability(capabilities, modules.privacyShield === true, "monitoring");
  pushCapability(capabilities, privacyCleanEnabled, "safeguards");

  return {
    density: mapExperienceLevelToDensity(experienceLevel),
    capabilities: Array.from(capabilities),
  };
}

export function getDependencyIds(
  dependencies: DependencyInfo[] | null | undefined,
): Set<string> {
  const normalize = (value: unknown): string[] => {
    if (typeof value !== "string") return [];
    const trimmed = value.trim();
    return trimmed ? [trimmed.toLowerCase()] : [];
  };

  return new Set(
    (dependencies ?? [])
      .filter((dependency) => dependency.installed)
      .flatMap((dependency) => {
        // KT: dependency status is backend/runtime data; older cache rows may
        // have null display fields, so normalize at the visibility boundary.
        return [
          ...normalize(dependency.id),
          ...normalize(dependency.panelId),
          ...normalize(dependency.name),
        ];
      }),
  );
}
