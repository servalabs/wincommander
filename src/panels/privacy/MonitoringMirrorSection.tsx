// src/panels/privacy/MonitoringMirrorSection.tsx
//
// My Monitoring Mirror — employee-facing disclosure of what leaves this
// machine (Pro). It renders the transmission state of each workplace monitor.
//
// The monitors are NOT switchable from here. Monitoring on a fleet-enrolled
// device is unconditional: the lawful basis is the employment agreement, not a
// per-device opt-in, so there is no consent gate and no employee kill-switch.
// This section exists to DISCLOSE, not to control.
//
// WHAT IS TRANSMITTED (fleet-enrolled devices only — keep this list true; it
// is the whole point of the section, and it is read by the person being
// monitored). The productivity-detail collector reads this device's
// ActivityWatch instance directly and reports: application names, full window
// titles, full URLs and page titles, source-file paths, project and language
// names, aggregate input counts, the activity timeline, idle periods, and the
// interactive username. See commander-pro/src/productivity_detail.rs.
//
// STILL NEVER COLLECTED: keystroke content (input is counts only), screenshots,
// webcam frames, clipboard contents, and file CONTENTS as distinct from paths.
//
// An earlier version of this header claimed window titles, URLs, paths and
// usernames never left the device. That was false — and it was false even
// before collection broadened, because the detail collector never consulted
// the monitor switches this section used to render.
//
// Backend: commander-pro/src/fleet_push.rs::monitoring_mirror (non-draining
//   peek of argus_signals + the last-sent ProductivitySample summary + the
//   transmission status).
// Free wrapper + registration: commander-free/src/lib.rs (argus_monitoring_mirror).
// Cross-slice: needs the `argus_monitoring_mirror` feature_id dispatch arm in
//   commander-pro/src/handlers.rs → fleet_push::monitoring_mirror.

import { useCallback, useEffect, useState } from 'react';
import { Button, Icon, Spinner } from '@/components/ui/bp';
import argus from '@/hooks/useArgus';
import { getFleetStatus } from '@/hooks/fleetStatus';
import { useLicenseQuery } from '@/hooks/queries/useLicenseQuery';
import { authAnomalyStatus, sessionMonitorStatus } from '@/hooks/monitorStatus';
import SectionCard from '../../components/shared/SectionCard';

// Response shapes (MonitoringMirror + friends) are hand-typed in useArgus.ts —
// the Free wrapper forwards opaque JSON from fleet_push::monitoring_mirror.

// ── Helpers ──────────────────────────────────────────────────────────────────

type MonitorStates = {
  sessionAssurance: boolean;
  accessSession: boolean;
  appUsage: boolean;
};


const INITIAL_MONITOR_STATES: MonitorStates = {
  sessionAssurance: false,
  accessSession: false,
  appUsage: false,
};

type MonitorStatusCardProps = {
  icon: 'eye-open' | 'shield' | 'desktop';
  title: string;
  description: string;
  running: boolean;
  transmissionAllowed: boolean;
};

function MonitorStatusCard({
  icon,
  title,
  description,
  running,
  transmissionAllowed,
}: MonitorStatusCardProps) {
  const sending = running && transmissionAllowed;
  const status = sending ? 'SENDING' : running ? 'LOCAL ONLY' : 'OFF';
  const statusClass = sending ? 'is-active' : running ? 'is-local' : 'is-off';
  const detail = sending
    ? 'Aggregate status may be sent.'
    : running
      ? 'Collecting locally; transmission is not active.'
      : 'Not collecting or transmitting.';

  return (
    <div className="employer-visibility-status-card">
      <div className="employer-visibility-status-card__top">
        <Icon icon={icon} size={14} color="var(--shield-text-muted)" />
        <span className={`employer-visibility-status-card__pill ${statusClass}`}>{status}</span>
      </div>
      <div className="employer-visibility-status-card__title">{title}</div>
      <p className="employer-visibility-status-card__copy">{description}</p>
      <span className="employer-visibility-status-card__detail">{detail}</span>
    </div>
  );
}

// ── Component ─────────────────────────────────────────────────────────────────

export default function MonitoringMirrorSection() {
  const { data: license, isLoading: licenseLoading } = useLicenseQuery();
  // The mirror is a Fleet surface, not a generic Pro surface. Calling
  // `fleet_status` for a normal-Pro licence asks the backend for a service the
  // user has not purchased, which used to produce a repeated entitlement error
  // every time this section mounted or refreshed.
  const fleetEntitled = license?.valid === true &&
    (license.active_service_features ?? license.features ?? []).includes('fleet');

  const [monitorStates, setMonitorStates] = useState<MonitorStates>(INITIAL_MONITOR_STATES);
  const [fleetConnected, setFleetConnected] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!fleetEntitled) {
      setFleetConnected(false);
      setMonitorStates(INITIAL_MONITOR_STATES);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const fleet = await getFleetStatus();
      setFleetConnected(fleet.connected);
      if (!fleet.connected) {
        setMonitorStates(INITIAL_MONITOR_STATES);
        return;
      }
      const [sessionAssurance, accessSession, appUsage] = await Promise.all([
        sessionMonitorStatus().catch(() => ({ running: false })),
        authAnomalyStatus().catch(() => ({ running: false })),
        argus.appUsageStatus().catch(() => ({ running: false })),
      ]);
      setMonitorStates({
        sessionAssurance: sessionAssurance.running === true,
        accessSession: accessSession.running === true,
        appUsage: appUsage.running === true,
      });
    } catch (e) {
      setFleetConnected(false);
      setMonitorStates(INITIAL_MONITOR_STATES);
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [fleetEntitled]);

  useEffect(() => {
    if (licenseLoading || !fleetEntitled) {
      setFleetConnected(false);
      setMonitorStates(INITIAL_MONITOR_STATES);
      return;
    }
    void refresh();
    const id = setInterval(() => void refresh(), 30_000);
    return () => clearInterval(id);
  }, [fleetEntitled, licenseLoading, refresh]);

  // ── Entitlement gate ─────────────────────────────────────────────────────

  if (!fleetEntitled || !fleetConnected) return null;

  const activeStatusCount = Object.values(monitorStates).filter(Boolean).length;
  const monitoringActive = activeStatusCount > 0;

  let statusPill: React.ReactNode;
  if (monitoringActive) {
    statusPill = (
      <span className="text-[10px] px-2 py-0.5 rounded bg-[var(--color-success)]/15 text-[var(--color-success)] border border-[var(--color-success)]/30 flex-shrink-0 font-mono">
        MONITORED
      </span>
    );
  } else {
    statusPill = (
      <span className="text-[10px] px-2 py-0.5 rounded bg-[var(--color-text-muted)]/10 text-[var(--color-text-muted)] border border-[var(--color-border)] flex-shrink-0 font-mono">
        NOT MONITORED
      </span>
    );
  }

  // ── Render ───────────────────────────────────────────────────────────────

  return (
    <SectionCard title="What my employer sees" icon="eye-open" headerRight={statusPill} armed={monitoringActive}>
      <div className="employer-visibility-card">
        <div className="employer-visibility-card__intro">
          <p>
            Workplace monitoring is active on this device under your employment agreement and cannot be
            switched off here. What is reported: application names, window titles, web addresses and page
            titles, file paths, project names, activity times and idle periods, and your username.
          </p>
          <p>
            Never reported: what you type (only counts of keystrokes and clicks), screenshots, webcam
            frames, clipboard contents, and the contents of your files.
          </p>
          <span className="employer-visibility-card__summary">
            {activeStatusCount} of 3 monitor{activeStatusCount === 1 ? '' : 's'} running
          </span>
        </div>

        <div className="employer-visibility-status-grid" aria-label="Workplace monitoring status">
          <MonitorStatusCard
            icon="eye-open"
            title="Session Assurance"
            description="Attention and session-risk signals."
            running={monitorStates.sessionAssurance}
            transmissionAllowed={fleetConnected}
          />
          <MonitorStatusCard
            icon="shield"
            title="Access & Session"
            description="Aggregate sign-in anomaly signals."
            running={monitorStates.accessSession}
            transmissionAllowed={fleetConnected}
          />
          <MonitorStatusCard
            icon="desktop"
            title="App Usage Monitor"
            description="Application, window-title and web activity detail."
            running={monitorStates.appUsage}
            transmissionAllowed={fleetConnected}
          />
        </div>

        <div className="employer-visibility-card__consent">
          <Icon icon={monitoringActive ? 'endorsed' : 'disable'} size={12} color="var(--shield-text-muted)" />
          <span>{monitoringActive ? 'Transmission is enabled.' : 'Transmission is not active on this machine.'}</span>
        </div>

        <div className="employer-visibility-card__actions">
          <Button icon="refresh" minimal small disabled={busy} onClick={() => void refresh()}>
            Refresh
          </Button>
          {busy && <Spinner size={14} />}
          {error && (
            <span role="alert" className="font-mono text-xs text-[var(--color-danger,#f87171)]">{error}</span>
          )}
        </div>
      </div>
    </SectionCard>
  );
}
