// src/panels/privacy/ArgusDlpSection.tsx
//
// Argus DLP Signal Monitor — data-loss-prevention signals (Pro).
//
// Surfaces aggregate DLP signals: large USB transfers, sensitive clipboard
// pattern hits, and coarse cloud-upload TCP activity. PRIVACY INVARIANT: only
// kind / class / magnitude / severity are shown — no filenames, paths, URLs,
// printer names, document names, or usernames cross the IPC boundary.
//
// Backend: commander-pro/src/dlp_monitor.rs
// Free wrappers: commander-free/src/argus.rs (start_argus_dlp etc.)
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

function formatMagnitude(entry: ArgusSignalEntry): string {
  const cls = entry.class;
  const mag = entry.magnitude;
  if (cls === 'usb_large_transfer' || cls === 'cloud_upload') {
    // magnitude = bytes
    if (mag >= 1_073_741_824) return `${(mag / 1_073_741_824).toFixed(1)} GiB`;
    if (mag >= 1_048_576) return `${(mag / 1_048_576).toFixed(1)} MiB`;
    if (mag >= 1024) return `${(mag / 1024).toFixed(1)} KiB`;
    return `${mag} B`;
  }
  if (cls === 'clipboard_sensitive') return `${mag} hit${mag !== 1 ? 's' : ''}`;
  return String(mag);
}

// ── Component ──────────────────────────────────────────────────────────────

export default function ArgusDlpSection() {
  const { canUse } = useEntitlements();

  // Collector
  const [status, setStatus] = useState<ArgusCollectorStatus | null>(null);
  const [recent, setRecent] = useState<ArgusSignalEntry[]>([]);
  const [monitorBusy, setMonitorBusy] = useState(false);
  const [monitorError, setMonitorError] = useState<string | null>(null);

  const isRunning = status?.running === true;
  const statusLoading = status === null && monitorError === null;

  // ── Data refresh ───────────────────────────────────────────────────────

  const refreshStatus = useCallback(async () => {
    try {
      const [s, r] = await Promise.all([
        argus.dlpStatus(),
        argus.dlpRecent(),
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
    const id = setInterval(() => void refreshStatus(), isRunning ? 10_000 : 30_000);
    return () => clearInterval(id);
  }, [isRunning, refreshStatus]);

  // ── Collector actions ──────────────────────────────────────────────────

  const toggleCollector = useCallback(async (on: boolean) => {
    setMonitorBusy(true);
    setMonitorError(null);
    try {
      if (on) {
        await argus.dlpStart();
      } else {
        await argus.dlpStop();
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
  } else if (isRunning) {
    statusPill = (
      <span className="text-[10px] px-2 py-0.5 rounded bg-[var(--color-success)]/15 text-[var(--color-success)] border border-[var(--color-success)]/30 flex-shrink-0 font-mono">
        ACTIVE
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
      <SectionCard title="DLP Signal Monitor" icon="shield" headerRight={<Tag minimal intent="none" className="font-mono text-[10px] flex-shrink-0">PRO</Tag>}>
        <p className="text-xs text-[var(--shield-text-subtle)] opacity-50">
          Aggregate data-loss-prevention signals: USB transfers, clipboard pattern hits, cloud-upload activity. Requires Pro.
        </p>
      </SectionCard>
    );
  }

  // ── Render ─────────────────────────────────────────────────────────────

  return (
    <SectionCard title="DLP Signal Monitor" icon="shield" headerRight={statusPill} armed={isRunning}>
      {/* ── Collector controls ── */}
      {(
        <div className="flex flex-col gap-3">
          <p className="text-xs text-[var(--shield-text-subtle)] text-pretty max-w-[360px]">
            Detects large USB transfers, sensitive clipboard pattern matches, and coarse
            cloud-upload activity. Only aggregate counts and byte totals are recorded -
            no filenames, paths, URLs, or clipboard content cross the IPC boundary.
          </p>
          {/* Start / Stop row */}
          <div className="flex items-center gap-3 flex-wrap">
            <Switch
              checked={isRunning}
              disabled={monitorBusy || statusLoading}
              onChange={(e) => void toggleCollector((e.target as HTMLInputElement).checked)}
              label="DLP monitoring active"
            />
            <Button
              icon="refresh"
              minimal
              small
              disabled={monitorBusy || statusLoading}
              onClick={() => void refreshStatus()}
              aria-label="Refresh DLP signals"
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
              <span>Running — started {formatRelative(status.startedAt)}</span>
            </div>
          )}

          {recent.length > 0 && (
            <ArgusSignalTable entries={recent} title="Recent DLP signals" formatMagnitude={formatMagnitude} />
          )}

          {recent.length === 0 && isRunning && (
            <div className="text-xs opacity-60">
              No DLP signals recorded yet — data appears after the first collection interval.
            </div>
          )}

        </div>
      )}
    </SectionCard>
  );
}
