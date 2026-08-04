import { getRadarDriftToggles, getRadarRecommendationToggles, getRadarToggles } from "../registry";
import { getToggleDrift, isToggleCheckedValue } from "./toggleDrift";
import { getPersona, type AppSettings } from "../types/settings";
import { getByPath, resolveToggleText, type ToggleDef } from "../types/toggles";
import { isModuleEnabled, type ModuleId } from "../types/modules";
import type { ScanFinding, ScanReport } from "../components/startup/WizardAnimations";

interface RadarSystemInfo {
  cpuUsage?: number;
  ramUsage?: number;
  disks?: Array<{ usedGb?: number }>;
}

interface RadarBlocklistStatus {
  applied?: string[] | null;
}

interface RadarBrowser {
  Name: string;
  Hardened: boolean;
}

interface BuildRadarReportInput {
  appSettings: AppSettings;
  systemInfo?: RadarSystemInfo | null;
  networkBlocklistStatus?: RadarBlocklistStatus | null;
  browserHardening?: RadarBrowser[] | null;
}

const MAINTENANCE_STALE_DAYS = 15;

function isRadarToggleActive(value: unknown, checkedWhen?: string): boolean {
  // null  → feature not supported on this system → treated as inactive
  // undefined → setting never configured by user → treated as inactive
  if (value === null || value === undefined) return false;
  if (checkedWhen !== undefined) return value === checkedWhen;
  return value === true;
}

function getRadarModuleForToggle(toggle: Pick<ToggleDef, "domain">): ModuleId | null {
  if (toggle.domain === "privacy") return "privacy";
  if (toggle.domain === "network") return "network";
  if (toggle.domain === "security" || toggle.domain === "tweaks") return "tweaks";
  return null;
}

function shouldIncludeRadarToggle(
  toggle: ToggleDef,
  appSettings: AppSettings,
): boolean {
  const moduleId = getRadarModuleForToggle(toggle);
  if (moduleId && !isModuleEnabled(appSettings.app.modules, moduleId)) return false;
  if (toggle.radarRequiresAntiCleanup) {
    return getPersona(appSettings) === "secure";
  }
  return true;
}

function isMaintenanceRunFresh(appSettings: AppSettings, key: string): boolean {
  const info = appSettings.ideal.tweaks?.maintenanceRuns?.[key];
  if (!info?.lastRunAt) return false;
  const lastRun = new Date(info.lastRunAt);
  if (Number.isNaN(lastRun.getTime())) return false;
  const ageDays = (Date.now() - lastRun.getTime()) / 86_400_000;
  return ageDays <= MAINTENANCE_STALE_DAYS;
}

export function shouldProbeBrowserHardening(appSettings: AppSettings | null | undefined): boolean {
  return isModuleEnabled(appSettings?.app?.modules, "privacy");
}

export function buildRadarReport({
  appSettings,
  systemInfo,
  networkBlocklistStatus,
  browserHardening,
}: BuildRadarReportInput): ScanReport {
  const findings: ScanFinding[] = [];
  const emittedIds = new Set<string>();

  const radarToggles = [
    ...getRadarToggles(),
    ...getRadarRecommendationToggles(),
  ];

  for (const toggle of radarToggles) {
    if (!shouldIncludeRadarToggle(toggle, appSettings)) {
      continue;
    }

    const value = getByPath(appSettings, toggle.currentPath);
    // A fresh install serializes every `current.*` as null (the Rust
    // Option<bool> defaults to None → JSON null), and the system probe only
    // overwrites the fields it covers. Blanket-skipping null therefore hid
    // every recommended default on a fresh box until a full probe ran — the
    // reported "only the engine finding shows" bug. So: for safe recommended
    // defaults, treat null like "inactive" and surface the nudge; for other
    // toggles keep skipping null (a genuinely unsupported/undetectable feature
    // shouldn't nag).
    const isSafeRecommendation = Boolean(toggle.safeDefault || toggle.defaultOn);
    if (value === null && !isSafeRecommendation) continue;

    const wording = resolveToggleText(toggle, appSettings.app.experienceLevel);

    if (!isRadarToggleActive(value, toggle.checkedWhen)) {
      const drift = getToggleDrift(appSettings, toggle);
      findings.push({
        id: toggle.id,
        category: toggle.radarCategory ?? "privacy",
        label: wording.label,
        impact: drift
          ? "User intent is ON, but Windows reports it OFF. Re-apply the desired setting."
          : toggle.impact ?? wording.description,
        severity: drift ? toggle.radarSeverity ?? "warning" : toggle.radarSeverity ?? "info",
        safeDefault: toggle.safeDefault,
        drift: drift !== null,
        targetChecked: drift?.targetChecked,
      });
      emittedIds.add(toggle.id);
    }
  }

  for (const toggle of getRadarDriftToggles()) {
    if (!shouldIncludeRadarToggle(toggle, appSettings)) {
      continue;
    }

    const idealRaw = getByPath(appSettings, toggle.settingsPath);
    if (idealRaw === null || idealRaw === undefined) continue;

    const currentRaw = getByPath(appSettings, toggle.currentPath);
    if (currentRaw === null || currentRaw === undefined) continue;

    const idealChecked = isToggleCheckedValue(idealRaw, toggle.checkedWhen);
    const currentChecked = isToggleCheckedValue(currentRaw, toggle.checkedWhen);
    if (idealChecked === currentChecked) continue;

    const wording = resolveToggleText(toggle, appSettings.app.experienceLevel);
    const id = idealChecked ? toggle.id : `drift:${toggle.id}`;
    if (emittedIds.has(id)) continue;

    findings.push({
      id,
      category: toggle.radarCategory ?? (toggle.domain === "tweaks" ? "performance" : "privacy"),
      label: idealChecked ? wording.label : `Revert ${wording.label}`,
      impact: idealChecked
        ? `User intent is ON, but Windows reports it OFF. Re-apply the desired setting.`
        : `User intent is OFF, but Windows reports it ON. Revert to the desired setting.`,
      severity: toggle.radarSeverity ?? "warning",
      safeDefault: toggle.safeDefault,
      drift: true,
      targetChecked: idealChecked,
    });
    emittedIds.add(id);
  }

  const isTelemetryBlocklistApplied = networkBlocklistStatus?.applied?.includes("telemetry-blocklist");
  if (
    isModuleEnabled(appSettings.app.modules, "network") &&
    networkBlocklistStatus &&
    !isTelemetryBlocklistApplied
  ) {
    findings.push({
      id: "telemetry-blocklist",
      category: "privacy",
      label: "Telemetry Blocklist",
      impact: "Domain-level tracking blocklist is inactive",
      severity: "warning",
    });
  }

  if (shouldProbeBrowserHardening(appSettings)) {
    const supportedBrowsers = (browserHardening ?? []).filter((browser) => !browser.Name.toLowerCase().includes("opera"));
    for (const browser of supportedBrowsers) {
      if (browser.Hardened) continue;
      findings.push({
        id: `browser-hardening:${browser.Name}`,
        category: "privacy",
        label: `${browser.Name} Hardening`,
        impact: `${browser.Name} still exposes telemetry, sync, cache, and tracking surfaces`,
        severity: "warning",
        safeDefault: true,
      });
    }
  }

  // Clipboard Secret Guard (PasteMonitorSection) is a bespoke monitor card,
  // not a ToggleDef, so it has no registry entry to drive it through the
  // toggle-based loop above. Secure-persona users are expected to have
  // credential-paste monitoring on; flag it here when it isn't.
  if (
    isModuleEnabled(appSettings.app.modules, "privacy") &&
    getPersona(appSettings) === "secure" &&
    !(appSettings.ideal?.privacy?.clipboard?.pasteMonitorEnabled ?? false)
  ) {
    findings.push({
      id: "paste-monitor",
      category: "privacy",
      label: "Clipboard Secret Guard",
      impact: "Copied passwords, API keys, and tokens aren't being watched for accidental exposure",
      severity: "warning",
      safeDefault: true,
    });
  }

  if (
    appSettings.app.firstRunComplete &&
    isModuleEnabled(appSettings.app.modules, "tweaks") &&
    !isMaintenanceRunFresh(appSettings, "services")
  ) {
    findings.push({
      id: "services-profile",
      category: "performance",
      label: "Service Profile",
      impact: "Windows background services have not been optimized recently",
      severity: "info",
      safeDefault: true,
    });
  }

  if (
    appSettings.app.firstRunComplete &&
    isModuleEnabled(appSettings.app.modules, "cleanup") &&
    !isMaintenanceRunFresh(appSettings, "cleanup")
  ) {
    findings.push({
      id: "disk-cleanup",
      category: "performance",
      label: "Windows Storage Cleanup",
      impact: "Windows temporary files and servicing leftovers have not been cleaned recently",
      severity: "info",
      safeDefault: true,
    });
  }

  return {
    findings,
    cpuUsage: systemInfo ? Math.round(systemInfo.cpuUsage ?? 0) : 0,
    ramUsage: systemInfo ? Math.round(systemInfo.ramUsage ?? 0) : 0,
    diskReclaimableGb: systemInfo
      ? Math.round(Math.max(0.5, (systemInfo.disks?.[0]?.usedGb ?? 50) * 0.03) * 10) / 10
      : 0,
  };
}
