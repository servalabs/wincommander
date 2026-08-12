// src/panels/privacy/ArgusAppUsageSection.tsx
//
// Argus App-Usage Monitor — foreground-window tracking (Pro).
//
// Shows aggregate active/idle time and top category per window slot.
// Raw exe paths and window titles are NEVER sent to fleet — only
// aggregate category scores + active/idle seconds (PRIVACY INVARIANT).
//
// Backend: commander-pro/src/session_monitor.rs
// Free wrappers: commander-free/src/argus.rs (argus_app_usage_start etc.)
// Fleet wire: fleet_push.rs + session_monitor::take_pending_sample()

import { useCallback, useEffect, useState } from 'react';
import { Button, Icon, Spinner, Tag } from '@/components/ui/bp';
import useEntitlements from '@/hooks/useEntitlements';
import { argus, type ArgusAppUsageStatus, type ArgusWindowSlot } from '@/hooks/useArgus';
import ProductivityTimeline from './ProductivityTimeline';
import SectionCard from '../../components/shared/SectionCard';

export type { ArgusWindowSlot };

// ── Types ──────────────────────────────────────────────────────────────────

type ArgusStatus = ArgusAppUsageStatus;

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

// ── Component ──────────────────────────────────────────────────────────────

export default function ArgusAppUsageSection() {
  const { canUse } = useEntitlements();

  // Monitor
  const [status, setStatus] = useState<ArgusStatus | null>(null);
  const [recent, setRecent] = useState<ArgusWindowSlot[]>([]);
  const [monitorError, setMonitorError] = useState<string | null>(null);

  const isRunning = status?.running === true;
  const statusLoading = status === null && monitorError === null;

  // ── Data refresh ───────────────────────────────────────────────────────

  const refreshStatus = useCallback(async () => {
    try {
      const [s, r] = await Promise.all([
        argus.appUsageStatus(),
        argus.appUsageRecent(),
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
      <SectionCard title="App-Usage Monitor" icon="desktop" headerRight={<Tag minimal intent="none" className="font-mono text-[10px] flex-shrink-0">PRO</Tag>}>
        <p className="text-xs text-[var(--shield-text-subtle)] opacity-50">
          Aggregate app-category tracking for fleet productivity. Requires Pro.
        </p>
      </SectionCard>
    );
  }

  // ── Render ─────────────────────────────────────────────────────────────

  return (
    <SectionCard title="App-Usage Monitor" icon="desktop" headerRight={statusPill} armed={isRunning}>
      {/* ── Monitor controls ── */}
      {(
        <div className="flex flex-col gap-3">
          <p className="text-xs text-[var(--shield-text-subtle)] text-pretty max-w-[360px]">
            Starts automatically after this device enrolls in Fleet and stops when it
            disconnects. Reporting is managed by the Fleet connection; this view shows
            the monitor's current local status.
          </p>
          <div className="flex items-center gap-3 flex-wrap">
            <span className="inline-flex items-center gap-2 text-[11px] font-mono text-[var(--shield-text-muted)]">
              <Icon icon="endorsed" size={12} /> Fleet-managed reporting
            </span>
            <Button
              icon="refresh"
              minimal
              small
              disabled={statusLoading}
              onClick={() => void refreshStatus()}
              aria-label="Refresh app-usage windows"
            >
              Refresh
            </Button>
            {statusLoading && <Spinner size={14} />}
          </div>

          {monitorError && (
            <div role="alert" className="font-mono text-xs text-[var(--color-danger,#f87171)]">{monitorError}</div>
          )}

          {/* Status line */}
          {status && isRunning && status.startedAt && (
            <div className="flex items-center gap-2 text-xs text-[var(--shield-text-subtle)]">
              <Icon icon="time" size={11} color="var(--shield-text-muted)" />
              <span>Running — started {formatRelative(status.startedAt)}</span>
              {status.intervalMs && (
                <span className="opacity-50 font-mono">· {status.intervalMs / 1000}s poll</span>
              )}
            </div>
          )}

          {/* Recent windows — timeline view (no titles, no paths) */}
          <ProductivityTimeline windows={recent} />

          {recent.length === 0 && isRunning && (
            <div className="text-xs opacity-60">
              No windows recorded yet — data appears after the first collection interval.
            </div>
          )}

        </div>
      )}
    </SectionCard>
  );
}
