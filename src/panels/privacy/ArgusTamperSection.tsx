// src/panels/privacy/ArgusTamperSection.tsx
//
// Argus Tamper Detection — integrity monitoring (Pro).
//
// Surfaces aggregate tamper signals: evidence log clearance,
// revocations, Pro-binary hash mismatches, and unexpected sidecar exits.
// PRIVACY INVARIANT: only kind / class / magnitude / severity are shown —
// no filenames, paths, or usernames cross the IPC boundary.
//
// Backend: commander-pro/src/tamper_monitor.rs
// Free wrappers: commander-free/src/argus.rs (start_argus_tamper etc.)
// Tamper hook: commander-free/src/log.rs clear_log_records() calls
//   record_argus_tamper_event (dispatch, spawn, ignore error).
// Fleet wire: fleet_push.rs drains argus_signals::take_pending() each heartbeat.

import { useCallback, useEffect, useState } from 'react';
import { Button, Icon, Spinner, Switch, Tag } from '@/components/ui/bp';
import useEntitlements from '@/hooks/useEntitlements';
import { argus, type ArgusCollectorStatus, type ArgusSignalEntry } from '@/hooks/useArgus';
import SectionCard from '../../components/shared/SectionCard';
import ArgusSignalTable from './ArgusSignalTable';

// ── Constants ──────────────────────────────────────────────────────────────

// ── Helpers ────────────────────────────────────────────────────────────────

function formatRelative(iso: string): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return '';
  const diffSec = Math.max(0, Math.round((Date.now() - t) / 1000));
  if (diffSec < 60) return `${diffSec}s ago`;
  const m = Math.floor(diffSec / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  return `${Math.floor(h / 24)}d ago`;
}

function classLabel(cls: string): string {
  const labels: Record<string, string> = {
    log_cleared: 'Log Cleared',
    binary_mismatch: 'Binary Mismatch',
    sidecar_exit: 'Sidecar Exit',
  };
  return labels[cls] ?? cls;
}

// ── Component ──────────────────────────────────────────────────────────────

export default function ArgusTamperSection() {
  const { canUse } = useEntitlements();

  // Collector
  const [status, setStatus] = useState<ArgusCollectorStatus | null>(null);
  const [recent, setRecent] = useState<ArgusSignalEntry[]>([]);
  const [monitorBusy, setMonitorBusy] = useState(false);
  const [monitorError, setMonitorError] = useState<string | null>(null);

  const isRunning = status?.running === true;
  const statusLoading = status === null && monitorError === null;

  const highCount = recent.filter(
    (e) => e.severity === 'critical' || e.severity === 'high',
  ).length;

  // ── Data refresh ───────────────────────────────────────────────────────

  const refreshStatus = useCallback(async () => {
    try {
      const [s, r] = await Promise.all([
        argus.tamperStatus(),
        argus.tamperRecent(),
      ]);
      setStatus(s);
      setRecent(r);
      setMonitorError(null);
    } catch (e) {
      setMonitorError(String(e));
    }
  }, []);

  useEffect(() => {
    void refreshStatus();
    // Poll faster when running — tamper events are time-sensitive.
    const id = setInterval(() => void refreshStatus(), isRunning ? 8_000 : 30_000);
    return () => clearInterval(id);
  }, [isRunning, refreshStatus]);

  // ── Collector actions ──────────────────────────────────────────────────

  const toggleCollector = useCallback(async (on: boolean) => {
    setMonitorBusy(true);
    setMonitorError(null);
    try {
      if (on) {
        await argus.tamperStart();
      } else {
        await argus.tamperStop();
      }
      await refreshStatus();
    } catch (e) {
      setMonitorError(String(e));
    } finally {
      setMonitorBusy(false);
    }
  }, [refreshStatus]);

  // ── Status pill ────────────────────────────────────────────────────────

  let statusPill: React.ReactNode;
  if (statusLoading) {
    statusPill = <span className="text-[10px] px-2 py-0.5 rounded border border-[var(--color-border)] flex-shrink-0 font-mono">CHECKING</span>;
  } else if (monitorError && status === null) {
    statusPill = <span className="text-[10px] px-2 py-0.5 rounded border border-[var(--color-danger,#f87171)]/40 text-[var(--color-danger,#f87171)] flex-shrink-0 font-mono">ERROR</span>;
  } else if (highCount > 0) {
    statusPill = (
      <span className="text-[10px] px-2 py-0.5 rounded bg-[var(--color-danger,#f87171)]/15 text-[var(--color-danger,#f87171)] border border-[var(--color-danger,#f87171)]/40 flex-shrink-0 font-mono">
        {highCount} TAMPER EVENT{highCount !== 1 ? 'S' : ''}
      </span>
    );
  } else if (isRunning) {
    statusPill = (
      <span className="text-[10px] px-2 py-0.5 rounded bg-[var(--color-success)]/15 text-[var(--color-success)] border border-[var(--color-success)]/30 flex-shrink-0 font-mono">
        WATCHING
      </span>
    );
  } else {
    statusPill = (
      <span className="text-[10px] px-2 py-0.5 rounded bg-[var(--color-text-muted)]/10 text-[var(--color-text-muted)] border border-[var(--color-border)] flex-shrink-0 font-mono">
        OFF
      </span>
    );
  }

  // ── Entitlement gate ───────────────────────────────────────────────────

  if (!canUse('paid')) {
    return (
      <SectionCard title="Tamper Detection" icon="shield" headerRight={<Tag minimal intent="none" className="font-mono text-[10px] flex-shrink-0">PRO</Tag>}>
        <p className="text-xs text-[var(--shield-text-subtle)] opacity-50">
          Detect evidence log clearance, binary integrity failures, and unexpected sidecar exits. Requires Pro.
        </p>
      </SectionCard>
    );
  }

  // ── Render ─────────────────────────────────────────────────────────────

  return (
    <SectionCard title="Tamper Detection" icon="shield" headerRight={statusPill} armed={isRunning || highCount > 0}>
      {/* ── Collector controls ── */}
      {(
        <div className="flex flex-col gap-3">
          <p className="text-xs text-[var(--shield-text-subtle)] text-pretty max-w-[360px]">
    Watches for evidence log clearance, Pro-binary hash
            mismatches, and unexpected sidecar exits. Only the event class and count
            are recorded - no file paths or usernames.
          </p>
          {/* Start / Stop row */}
          <div className="flex items-center gap-3 flex-wrap">
            <Switch
              checked={isRunning}
              disabled={monitorBusy || statusLoading}
              onChange={(e) => void toggleCollector((e.target as HTMLInputElement).checked)}
              label="Tamper detection active"
            />
            <Button
              icon="refresh"
              minimal
              small
              disabled={monitorBusy || statusLoading}
              onClick={() => void refreshStatus()}
              aria-label="Refresh tamper events"
            >
              Refresh
            </Button>
            {(monitorBusy || statusLoading) && <Spinner size={14} />}
          </div>

          {monitorError && (
            <div role="alert" className="font-mono text-xs text-[var(--color-danger,#f87171)]">{monitorError}</div>
          )}

          {/* Status line */}
          {status && isRunning && status.startedAt && (
            <div className="flex items-center gap-2 text-xs text-[var(--shield-text-subtle)]">
              <Icon icon="time" size={11} color="var(--shield-text-muted)" />
              <span>Watching — started {formatRelative(status.startedAt)}</span>
            </div>
          )}

          {recent.length > 0 && (
            <ArgusSignalTable entries={recent} title="Recent tamper events" formatClass={classLabel} formatMagnitude={(entry) => String(entry.magnitude)} />
          )}

          {recent.length === 0 && isRunning && (
            <div className="text-xs opacity-60">
              No tamper events detected — integrity looks clean.
            </div>
          )}

        </div>
      )}
    </SectionCard>
  );
}
