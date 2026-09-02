// src/panels/privacy/DriverHealthSection.tsx
//
// "Device & Driver Health" — a read-only check that answers the question a
// user would otherwise open Device Manager for: is anything missing a driver
// or malfunctioning? It enumerates PnP devices in the Pro sidecar
// (`Get-DriverHealth`), surfaces only the unhealthy set, maps the raw
// Device-Manager problem code to plain-English text + severity, and lists
// them sorted critical → warning → info.
//
// Conservative false-positive posture: user-disabled (code 22) and
// not-connected (code 45) devices are `info` — shown but visually muted,
// excluded from the headline problem count, and never toasted.
//
// The optional "Watch" switch (expert density only) starts a low-frequency
// re-scan in the sidecar that fires a native toast the first time a NEW
// critical device appears. The toggle persists in settings so
// BackgroundPollers can auto-start it at launch.
//
// Re-homed 2026-06-09 from the retired Intelligence panel into Privacy ▸
// Alerts & Monitoring (redesign IA-02 "Device health"). The only change from
// the original is that the expert gate now comes in as an `isAdvanced` prop
// from the parent's density resolver instead of the removed
// `app.experienceLevel` field.

import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button, Spinner, Switch, Tag } from '@/components/ui/bp';
import type { Intent } from '@/components/ui/bp';
import SectionCard from '../../components/shared/SectionCard';
import { useAppState } from '../../context/AppContext';
import { isPrivilegedWriteBlocked, MACHINE_SCOPE_ELEVATION_MESSAGE } from '../../lib/machineScopeElevation';
import { reportSettingsWriteFailure } from '../../lib/settingsWriteRecovery';
import {
  driverHealthIgnoreId,
  ignoredDriverFindingCount,
  isIgnoredDriverFinding,
  vulnerableDriverIgnoreId,
} from './driverFindingIgnore';
import './DriverHealthSection.css';

type Severity = 'critical' | 'warning' | 'info';

interface DriverProblem {
  name: string;
  class: string;
  status: string;
  problemCode: number | null;
  problemText: string;
  severity: Severity;
  instanceId: string;
  manufacturer: string;
}

interface DriverHealthSummary {
  total: number;
  critical: number;
  warning: number;
  info: number;
  ok: boolean;
}

interface DriverHealthReport {
  devices: DriverProblem[];
  summary: DriverHealthSummary;
}

// ── BYOVD types ──────────────────────────────────────────────────────

interface VulnerableDriver {
  filename: string;
  path: string;
  state: string;
  reason: string;
  matchedBy: string;
}

interface VulnerableDriversReport {
  vulnerable: VulnerableDriver[];
  scanned: number;
  ok: boolean;
}

interface DriverHealthSectionProps {
  /** Expert density unlocks the optional background "Watch" switch. */
  isAdvanced?: boolean;
  /** Used by Maintenance's unified Drivers card. */
  embedded?: boolean;
  /** Bumps when the parent-wide scan action runs. */
  scanKey?: number;
  /** Keeps parent-level scanning as the single explicit scan action. */
  hideActions?: boolean;
}

const SEVERITY_RANK: Record<Severity, number> = { critical: 0, warning: 1, info: 2 };

function severityIntent(sev: Severity): Intent | undefined {
  if (sev === 'critical') return 'danger';
  if (sev === 'warning') return 'warning';
  return undefined; // info → minimal/muted
}

/** One-line remediation hint keyed off problem code / severity. */
function remediation(d: DriverProblem): string {
  switch (d.problemCode) {
    case 28:
      return "Install the driver: Open Device Manager → right-click → Update driver, or get it from the vendor's site.";
    case 43:
    case 10:
    case 31:
      return 'Try Open Device Manager → Disable then Enable the device, or reinstall its driver. A reboot often clears code 43.';
    case 22:
      return 'You disabled this device. Re-enable it in Device Manager if you want it back.';
    case 45:
      return "Device isn't connected right now.";
    default:
      if (d.severity === 'info') return 'No action needed — this is an expected or transient state.';
      return 'Open Device Manager for details and to update/reinstall the driver.';
  }
}

export default function DriverHealthSection({ isAdvanced = false, embedded = false, scanKey = 0, hideActions = false }: DriverHealthSectionProps) {
  const { appSettings, patchAppSettings, systemInfo } = useAppState();
  const needsElevation = isPrivilegedWriteBlocked(true, systemInfo?.isAdmin);
  const watchEnabled = appSettings?.ideal?.security?.drivers?.watchEnabled ?? false;
  const watchIntervalSecs = appSettings?.ideal?.security?.drivers?.watchIntervalSecs ?? 60;
  const ignoredFindingIds = appSettings?.app?.ignoredFindingIds ?? [];

  const [report, setReport] = useState<DriverHealthReport | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [watchBusy, setWatchBusy] = useState(false);

  // ── BYOVD state ───────────────────────────────────────────────────
  const [vulnReport, setVulnReport] = useState<VulnerableDriversReport | null>(null);
  const [vulnLoading, setVulnLoading] = useState(false);
  const [vulnError, setVulnError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const r = await invoke<DriverHealthReport>('get_driver_health');
      if (!r?.summary || !Array.isArray(r.devices)) {
        throw new Error('Driver health returned an invalid response.');
      }
      setReport(r);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  const refreshVuln = useCallback(async () => {
    setVulnLoading(true);
    setVulnError(null);
    try {
      const r = await invoke<VulnerableDriversReport>('get_vulnerable_drivers');
      if (!Array.isArray(r?.vulnerable)) {
        throw new Error('Vulnerable-driver scan returned an invalid response.');
      }
      setVulnReport(r);
    } catch (err) {
      setVulnError(String(err));
    } finally {
      setVulnLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    void refreshVuln();
  }, [refresh, refreshVuln, scanKey]);

  const handleOpenDeviceManager = useCallback(async () => {
    try {
      await invoke('open_device_manager');
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const handleWatchToggle = useCallback(
    async (next: boolean) => {
      if (needsElevation) { setError(MACHINE_SCOPE_ELEVATION_MESSAGE); return; }
      setWatchBusy(true);
      setError(null);
      try {
        if (next) {
          await invoke('start_driver_watch', { intervalSecs: watchIntervalSecs });
        } else {
          await invoke('stop_driver_watch');
        }
        await patchAppSettings({
          ideal: { security: { drivers: { watchEnabled: next } } },
        }).catch(reportSettingsWriteFailure);
      } catch (err) {
        setError(String(err));
      } finally {
        setWatchBusy(false);
      }
    },
    [patchAppSettings, watchIntervalSecs, needsElevation],
  );

  const handleWatchIntervalChange = useCallback(async (next: number) => {
    if (needsElevation) { setError(MACHINE_SCOPE_ELEVATION_MESSAGE); return; }
    const bounded = Math.max(30, Math.min(3600, next));
    await patchAppSettings({
      ideal: { security: { drivers: { watchIntervalSecs: bounded } } },
    }).catch(reportSettingsWriteFailure);
    if (watchEnabled) {
      await invoke('start_driver_watch', { intervalSecs: bounded }).catch((err) => {
        setError(String(err));
      });
    }
  }, [patchAppSettings, watchEnabled, needsElevation]);

  const summary = report?.summary;
  const sortedDevices = report
    ? [...report.devices]
      .filter((driver) => !isIgnoredDriverFinding(ignoredFindingIds, driverHealthIgnoreId(driver)))
      .sort((a, b) => SEVERITY_RANK[a.severity] - SEVERITY_RANK[b.severity])
    : [];
  const problemCount = sortedDevices.filter((driver) => driver.severity === 'critical' || driver.severity === 'warning').length;
  const visibleVulnerableDrivers = vulnReport?.vulnerable.filter(
    (driver) => !isIgnoredDriverFinding(ignoredFindingIds, vulnerableDriverIgnoreId(driver)),
  ) ?? [];
  const ignoredDriverCount = ignoredDriverFindingCount(ignoredFindingIds);

  const ignoreDriverFinding = useCallback((id: string) => {
    void patchAppSettings((latest) => ({
      app: { ignoredFindingIds: [...new Set([...(latest?.app?.ignoredFindingIds ?? []), id])] },
    })).catch(reportSettingsWriteFailure);
  }, [patchAppSettings]);

  const restoreDriverFindings = useCallback(() => {
    void patchAppSettings((latest) => ({
      app: {
        ignoredFindingIds: (latest?.app?.ignoredFindingIds ?? []).filter(
          (id) => !id.startsWith('driver-health:') && !id.startsWith('driver-byovd:'),
        ),
      },
    })).catch(reportSettingsWriteFailure);
  }, [patchAppSettings]);

  const headerTag = summary ? (
    summary.ok || problemCount === 0 ? (
      <Tag minimal intent="success" className="font-mono">
        ALL DEVICES OK
      </Tag>
    ) : (
      <Tag minimal intent={summary.critical > 0 ? 'danger' : 'warning'} className="font-mono">
        {problemCount} PROBLEM{problemCount === 1 ? '' : 'S'}
      </Tag>
    )
  ) : null;

  const body = <div className="driver-health-body">
        <div className="driver-health-intro">
          Queries Windows PnP / Device Manager for hardware that is missing a driver or
          malfunctioning. Devices you intentionally disabled are shown but not flagged.
        </div>

        <div className="driver-health-controls">
          {!hideActions && <Button icon="refresh" minimal small onClick={refresh} disabled={loading}>Refresh</Button>}
          <Button icon="cog" minimal small onClick={handleOpenDeviceManager}>
            Open Device Manager
          </Button>
          {loading && <Spinner size={14} />}
          {isAdvanced && (
            <Switch
              checked={watchEnabled}
              disabled={watchBusy || needsElevation}
              onChange={(e) => handleWatchToggle((e.target as HTMLInputElement).checked)}
              label="Watch for new driver problems"
              className="driver-health-watch-switch"
            />
          )}
          {isAdvanced && watchEnabled && (
            <label className="flex items-center gap-2 text-[11px]">
              Check every
              <select
                value={watchIntervalSecs}
                onChange={(e) => void handleWatchIntervalChange(Number(e.currentTarget.value))}
                aria-label="Driver health watch interval"
                disabled={needsElevation}
                className="rounded border border-[var(--shield-inner-border)] bg-[var(--color-bg-secondary)] px-2 py-1"
              >
                <option value={30}>30 seconds</option>
                <option value={60}>1 minute</option>
                <option value={300}>5 minutes</option>
                <option value={900}>15 minutes</option>
                <option value={3600}>1 hour</option>
              </select>
            </label>
          )}
          {ignoredDriverCount > 0 && <Button icon="reset" minimal small onClick={restoreDriverFindings}>Restore ignored ({ignoredDriverCount})</Button>}
        </div>

        {needsElevation && isAdvanced && <div role="alert" className="driver-health-error">{MACHINE_SCOPE_ELEVATION_MESSAGE}</div>}

        {error && <div role="alert" className="driver-health-error">{error}</div>}

        {!loading && summary && (summary.ok || sortedDevices.length === 0) && (
          <div className="driver-health-empty">No driver problems detected.</div>
        )}

        {sortedDevices.length > 0 && (
          <div className="driver-health-list">
            {sortedDevices.map((d) => (
              <div
                key={d.instanceId || `${d.name}-${d.problemCode}`}
                className={`driver-health-row is-${d.severity}`}
              >
                <div className="driver-health-row-head">
                  <span className="driver-health-row-name">{d.name || 'Unknown device'}</span>
                  {d.class && (
                    <Tag minimal className="font-mono">
                      {d.class}
                    </Tag>
                  )}
                  <Tag minimal={d.severity === 'info'} intent={severityIntent(d.severity)} className="font-mono">
                    {d.severity.toUpperCase()}
                    {d.problemCode != null ? ` · CODE ${d.problemCode}` : ''}
                  </Tag>
                  {d.severity === 'critical' && <Button icon="eye-off" minimal small onClick={() => ignoreDriverFinding(driverHealthIgnoreId(d))}>Ignore</Button>}
                </div>
                <div className="driver-health-row-problem">{d.problemText}</div>
                <div className="driver-health-row-meta">
                  <span className="driver-health-instance">{d.instanceId || '—'}</span>
                  {d.manufacturer && <span>{d.manufacturer}</span>}
                </div>
                <div className="driver-health-row-hint">{remediation(d)}</div>
              </div>
            ))}
          </div>
        )}

        {/* ── BYOVD subsection ─────────────────────────────────────── */}
        <div className="driver-health-byovd">
          <div className="driver-health-byovd-header">
            <div className="driver-health-byovd-heading">
              <span className="driver-health-byovd-eyebrow">Kernel exposure check</span>
              <span className="driver-health-byovd-title">Vulnerable drivers (BYOVD)</span>
            </div>
            <div className="driver-health-byovd-actions">
              {vulnLoading && <Spinner size={14} />}
              {vulnReport && (
                <Tag
                  minimal={vulnReport.ok}
                  intent={vulnReport.ok || visibleVulnerableDrivers.length === 0 ? 'success' : 'danger'}
                  className="font-mono"
                >
                  {vulnReport.ok ? 'NO EXPOSURE FOUND' : visibleVulnerableDrivers.length === 0 ? 'ALL IGNORED' : `${visibleVulnerableDrivers.length} FOUND`}
                </Tag>
              )}
              {!hideActions && <Button icon="refresh" minimal small onClick={refreshVuln} disabled={vulnLoading}>Refresh scan</Button>}
            </div>
          </div>

          <div className="driver-health-byovd-desc">
            Checks loaded kernel drivers against a curated subset of <span className="font-mono">loldrivers.io</span> — signed but known-abusable drivers used in BYOVD / EDR-killer attacks.
            {vulnReport && (
              <span className="driver-health-byovd-scanned">
                {vulnReport.scanned} drivers scanned
              </span>
            )}
          </div>

          {vulnError && (
            <div role="alert" className="driver-health-error">{vulnError}</div>
          )}

              {!vulnLoading && vulnReport && (vulnReport.ok || visibleVulnerableDrivers.length === 0) && (
            <div className="driver-health-byovd-empty">No active known-vulnerable drivers are shown.</div>
          )}

          {vulnReport && visibleVulnerableDrivers.length > 0 && (
            <div className="driver-health-list">
              {visibleVulnerableDrivers.map((v) => (
                <div key={v.filename} className="driver-health-row is-critical driver-health-byovd-row">
                  <div className="driver-health-row-head">
                    <span className="driver-health-row-name font-mono">{v.filename}</span>
                    <Tag intent="danger" className="font-mono">
                      BYOVD
                    </Tag>
                    <Tag minimal className="font-mono">
                      {v.state}
                    </Tag>
                    <Button icon="eye-off" minimal small onClick={() => ignoreDriverFinding(vulnerableDriverIgnoreId(v))}>Ignore</Button>
                  </div>
                  <div className="driver-health-row-problem">{v.reason}</div>
                  {v.path && (
                    <div className="driver-health-row-meta">
                      <span className="driver-health-instance">{v.path}</span>
                    </div>
                  )}
                  <div className="driver-health-row-hint">
                    This driver is loaded and known to expose kernel primitives abused by
                    ransomware and EDR-killers. Investigate whether it is legitimately required;
                    if not, unload it and remove the associated software.
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>;

  if (embedded) return body;
  return <SectionCard title="Device & Driver Health" icon="pulse" headerRight={headerTag}>{body}</SectionCard>;
}
