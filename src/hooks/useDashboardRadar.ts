/**
 * useDashboardRadar — REGISTRY-DRIVEN radar scan for the dashboard.
 *
 * BEFORE: Hardcoded 24-entry SCAN_CHECKS array with its own field mappings,
 *         duplicated in setup flows with slightly different entries.
 * AFTER:  Dashboard and Help & Setup both call the same shared registry-driven
 *         report builder. One scan definition, no drift, no second opinions.
 *
 * Special cases:
 *   - "telemetry-blocklist" is a network-level check, not a toggle — handled separately.
 */
import { useMemo } from 'react';
import { useEffect, useState } from 'react';
import { useAppState } from '../context/AppContext';
import type { ScanReport } from '../components/startup/WizardAnimations';
import { buildRadarReport, shouldProbeBrowserHardening } from '../lib/radarScan';
import { isModuleEnabled } from '../types/modules';
import { getPersona } from '../types/settings';
import {
  loadDiskCleanupSchedules,
  subscribeToDiskCleanupScheduleInvalidation,
  type DiskCleanupSchedule,
} from '../panels/maintenance/diskCleanupScheduleState';
import { executeBackendCommand, type BlocklistStatus, type InstalledBrowser, useBackend } from './useBackend';

export interface DashboardRadar {
  phase: 'idle' | 'scanning' | 'complete';
  report: ScanReport | null;
}

export function useDashboardRadar({ scheduledWipesEnabled = false }: { scheduledWipesEnabled?: boolean } = {}): DashboardRadar {
  const {
    systemInfo,
    networkBlocklistStatus: cachedNetworkBlocklistStatus,
    appSettings,
  } = useAppState();
  const { getAutoEraseSchedules } = useBackend();
  const [browserHardening, setBrowserHardening] = useState<InstalledBrowser[] | null>(null);
  const [autoEraseSchedules, setAutoEraseSchedules] = useState<DiskCleanupSchedule[] | undefined>();
  const [dashboardBlocklistStatus, setDashboardBlocklistStatus] = useState<BlocklistStatus | null>(
    cachedNetworkBlocklistStatus
  );

  // Dashboard should stop scanning as soon as settings are available.
  // Network blocklist probe can be delayed/null and is optional for report build.
  const hasData = appSettings !== null;
  const probeBrowserHardening = shouldProbeBrowserHardening(appSettings);
  const probeNetworkBlocklist = isModuleEnabled(appSettings?.app?.modules, "network");
  const probeAutoEraseSchedules = Boolean(
    scheduledWipesEnabled &&
    appSettings &&
    getPersona(appSettings) === "secure" &&
    isModuleEnabled(appSettings.app.modules, "cleanup"),
  );
  const browserHardeningFingerprint = [
    appSettings?.current?.tweaks?.security?.firefoxHardeningEnabled,
    appSettings?.current?.tweaks?.security?.braveHardeningEnabled,
    appSettings?.current?.tweaks?.security?.chromeHardeningEnabled,
    appSettings?.current?.tweaks?.security?.edgeHardeningEnabled,
  ].join("|");
  const networkBlocklistFingerprint = appSettings?.current?.network?.hosts?.enabledBlocklists?.join("|") ?? "";

  useEffect(() => {
    if (cachedNetworkBlocklistStatus) {
      setDashboardBlocklistStatus(cachedNetworkBlocklistStatus);
    }
  }, [cachedNetworkBlocklistStatus]);

  useEffect(() => {
    if (!hasData || !probeBrowserHardening) {
      setBrowserHardening(null);
      return;
    }

    let cancelled = false;

    void executeBackendCommand<{ browsers?: InstalledBrowser[] }>('Get-InstalledBrowsersJson')
      .then((result) => {
        if (cancelled) return;
        setBrowserHardening(result.success ? (result.data?.browsers ?? []) : []);
      })
      .catch(() => {
        if (!cancelled) setBrowserHardening([]);
      });

    return () => {
      cancelled = true;
    };
  }, [hasData, probeBrowserHardening, browserHardeningFingerprint]);

  useEffect(() => {
    if (!hasData || !probeNetworkBlocklist) {
      setDashboardBlocklistStatus(null);
      return;
    }

    let cancelled = false;

    void executeBackendCommand<BlocklistStatus>('Get-BlocklistStatus')
      .then((result) => {
        if (cancelled) return;
        setDashboardBlocklistStatus(result.success ? (result.data ?? null) : null);
      })
      .catch(() => {
        if (!cancelled) setDashboardBlocklistStatus(null);
      });

    return () => {
      cancelled = true;
    };
  }, [hasData, probeNetworkBlocklist, networkBlocklistFingerprint]);

  useEffect(() => {
    if (!probeAutoEraseSchedules) {
      setAutoEraseSchedules(undefined);
      return;
    }

    let cancelled = false;
    const refreshSchedules = () => {
      void loadDiskCleanupSchedules(getAutoEraseSchedules, true).then((result) => {
        if (cancelled) return;
        setAutoEraseSchedules(result.success && result.data ? result.data.schedules : undefined);
      });
    };
    refreshSchedules();
    const unsubscribe = subscribeToDiskCleanupScheduleInvalidation(refreshSchedules);

    return () => {
      cancelled = true;
      unsubscribe();
    };
  }, [getAutoEraseSchedules, probeAutoEraseSchedules]);

  const report = useMemo<ScanReport | null>(() => {
    if (!hasData) return null;
    return buildRadarReport({
      appSettings,
      networkBlocklistStatus: dashboardBlocklistStatus,
      systemInfo,
      browserHardening,
      autoEraseSchedules,
    });
  }, [
    systemInfo, hasData, dashboardBlocklistStatus, appSettings, browserHardening, autoEraseSchedules
  ]);

  const phase: DashboardRadar['phase'] = hasData ? 'complete' : 'scanning';

  return { phase, report };
}
