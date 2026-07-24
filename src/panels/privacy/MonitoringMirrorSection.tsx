// src/panels/privacy/MonitoringMirrorSection.tsx
//
// My Monitoring Mirror — employee-facing proof of exactly what leaves this
// machine (Pro). It renders the same aggregate scalars the fleet already
// receives and owns the three monitor switches.
//
// PRIVACY INVARIANT (see AGENTS.md): window titles, exe paths, URLs,
// filenames, printer names, document names, usernames, keystrokes,
// screenshots, and webcam frames NEVER leave the device and are NEVER shown
// here — this section only surfaces what the fleet actually collects.
//
// Backend: commander-pro/src/fleet_push.rs::monitoring_mirror (non-draining
//   peek of argus_signals + the last-sent ProductivitySample summary + the
//   transmission status).
// Free wrapper + registration: commander-free/src/lib.rs (argus_monitoring_mirror).
// Cross-slice: needs the `argus_monitoring_mirror` feature_id dispatch arm in
//   commander-pro/src/handlers.rs → fleet_push::monitoring_mirror.

import { useCallback, useEffect, useState } from 'react';
import { Button, Icon, Spinner, Switch } from '@/components/ui/bp';
import useEntitlements from '@/hooks/useEntitlements';
import argus from '@/hooks/useArgus';
import { getFleetStatus } from '@/hooks/fleetStatus';
import {
  authAnomalyStatus,
  sessionMonitorStatus,
  startAuthAnomalyMonitor,
  startSessionMonitor,
  stopAuthAnomalyMonitor,
  stopSessionMonitor,
} from '@/hooks/monitorStatus';
import SectionCard from '../../components/shared/SectionCard';

// Response shapes (MonitoringMirror + friends) are hand-typed in useArgus.ts —
// the Free wrapper forwards opaque JSON from fleet_push::monitoring_mirror.

// ── Helpers ──────────────────────────────────────────────────────────────────

type MonitorStates = {
  sessionAssurance: boolean;
  accessSession: boolean;
  appUsage: boolean;
};

type MonitorKey = keyof MonitorStates;

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
  busy: boolean;
  onToggle: (enabled: boolean) => void;
};

function MonitorStatusCard({
  icon,
  title,
  description,
  running,
  transmissionAllowed,
  busy,
  onToggle,
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
      <Switch
        checked={running}
        disabled={busy}
        onChange={(event) => onToggle((event.target as HTMLInputElement).checked)}
        label="Monitoring active"
        className="employer-visibility-status-card__switch"
      />
    </div>
  );
}

// ── Component ─────────────────────────────────────────────────────────────────

export default function MonitoringMirrorSection() {
  const { canUse } = useEntitlements();

  const [monitorStates, setMonitorStates] = useState<MonitorStates>(INITIAL_MONITOR_STATES);
  const [fleetConnected, setFleetConnected] = useState<boolean | null>(null);
  const [busyMonitor, setBusyMonitor] = useState<MonitorKey | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const fleet = await getFleetStatus();
      setFleetConnected(fleet.connected);
      if (!fleet.connected) {
        setMonitorStates(INITIAL_MONITOR_STATES);
        return;
      }
      const [, sessionAssurance, accessSession, appUsage] = await Promise.all([
        argus.monitoringMirror(),
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
  }, []);

  useEffect(() => {
    if (!canUse('paid')) {
      setFleetConnected(false);
      return;
    }
    void refresh();
    const id = setInterval(() => void refresh(), 30_000);
    return () => clearInterval(id);
  }, [canUse, refresh]);

  const toggleMonitor = useCallback(async (monitor: MonitorKey, enabled: boolean) => {
    setBusyMonitor(monitor);
    setError(null);
    try {
      if (monitor === "sessionAssurance") {
        await (enabled ? startSessionMonitor() : stopSessionMonitor());
      } else if (monitor === "accessSession") {
        await (enabled ? startAuthAnomalyMonitor() : stopAuthAnomalyMonitor());
      } else {
        await (enabled ? argus.appUsageStart() : argus.appUsageStop());
      }
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyMonitor(null);
    }
  }, [refresh]);

  // ── Entitlement gate ─────────────────────────────────────────────────────

  if (!canUse('paid') || !fleetConnected) return null;

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
            Status and controls for the workplace monitors on this device. Only aggregate counts and
            scores can be transmitted — never file names, URLs, keystrokes, screenshots, or webcam frames.
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
            busy={busyMonitor === "sessionAssurance"}
            onToggle={(enabled) => void toggleMonitor("sessionAssurance", enabled)}
          />
          <MonitorStatusCard
            icon="shield"
            title="Access & Session"
            description="Aggregate sign-in anomaly signals."
            running={monitorStates.accessSession}
            transmissionAllowed={fleetConnected}
            busy={busyMonitor === "accessSession"}
            onToggle={(enabled) => void toggleMonitor("accessSession", enabled)}
          />
          <MonitorStatusCard
            icon="desktop"
            title="App Usage Monitor"
            description="Aggregate active and idle time by category."
            running={monitorStates.appUsage}
            transmissionAllowed={fleetConnected}
            busy={busyMonitor === "appUsage"}
            onToggle={(enabled) => void toggleMonitor("appUsage", enabled)}
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
            <span className="font-mono text-xs text-[var(--color-danger,#f87171)]">{error}</span>
          )}
        </div>
      </div>
    </SectionCard>
  );
}
