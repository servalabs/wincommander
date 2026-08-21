// src/panels/privacy/ArgusPrintUsbSection.tsx
//
// Argus Print & Removable Media Monitor — print + USB signal (Pro).
//
// Surfaces aggregate print and USB signals: Windows PrintService EventID 307
// (page counts only — no document names) and USB mass-storage attach events
// (device class only — no filenames). PRIVACY INVARIANT: only kind / class /
// magnitude / severity are shown — no document names, printer names, paths,
// or usernames cross the IPC boundary.
//
// Backend: commander-pro/src/print_usb_monitor.rs
// Free wrappers: commander-free/src/argus.rs (start_argus_print_usb etc.)
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
  if (entry.class === 'print_job') {
    return `${entry.magnitude} page${entry.magnitude !== 1 ? 's' : ''}`;
  }
  if (entry.class === 'usb_attach') {
    return `${entry.magnitude} device${entry.magnitude !== 1 ? 's' : ''}`;
  }
  return String(entry.magnitude);
}

function classLabel(cls: string): string {
  const labels: Record<string, string> = {
    print_job: 'Print Job',
    usb_attach: 'USB Attach',
  };
  return labels[cls] ?? cls;
}

// ── Component ──────────────────────────────────────────────────────────────

export default function ArgusPrintUsbSection() {
  const { canUse } = useEntitlements();

  // Collector
  const [status, setStatus] = useState<ArgusCollectorStatus | null>(null);
  const [recent, setRecent] = useState<ArgusSignalEntry[]>([]);
  const [monitorBusy, setMonitorBusy] = useState(false);
  const [monitorError, setMonitorError] = useState<string | null>(null);

  const isRunning = status?.running === true;
  const statusLoading = status === null && monitorError === null;

  const printCount = recent.filter((e) => e.kind === 'print').length;
  const usbCount = recent.filter((e) => e.kind === 'removable_media').length;

  // ── Data refresh ───────────────────────────────────────────────────────

  const refreshStatus = useCallback(async () => {
    try {
      const [s, r] = await Promise.all([
        argus.printUsbStatus(),
        argus.printUsbRecent(),
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
        await argus.printUsbStart();
      } else {
        await argus.printUsbStop();
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
      <SectionCard title="Print & Removable Media Monitor" icon="print" headerRight={<Tag minimal intent="none" className="font-mono text-[10px] flex-shrink-0">PRO</Tag>}>
        <p className="text-xs text-[var(--shield-text-subtle)] opacity-50">
          Detect print jobs and USB mass-storage attach events — page counts and device
          classes only. Requires Pro.
        </p>
      </SectionCard>
    );
  }

  // ── Render ─────────────────────────────────────────────────────────────

  return (
    <SectionCard title="Print & Removable Media Monitor" icon="print" headerRight={statusPill} armed={isRunning}>
      {/* ── Collector controls ── */}
      {(
        <div className="flex flex-col gap-3">
          <p className="text-xs text-[var(--shield-text-subtle)] text-pretty max-w-[360px]">
            Detects print jobs (page count only) and USB mass-storage device attachments.
            Document names, printer names, and filenames are never recorded - only the
            aggregate page count and device-class label reach the fleet.
          </p>
          {/* Start / Stop row */}
          <div className="flex items-center gap-3 flex-wrap">
            <Switch
              checked={isRunning}
              disabled={monitorBusy || statusLoading}
              onChange={(e) => void toggleCollector((e.target as HTMLInputElement).checked)}
              label="Print &amp; USB monitoring active"
            />
            <Button
              icon="refresh"
              minimal
              small
              disabled={monitorBusy || statusLoading}
              onClick={() => void refreshStatus()}
              aria-label="Refresh print and USB signals"
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

          {/* Summary counters when data is present */}
          {recent.length > 0 && (
            <div className="flex items-center gap-4 flex-wrap">
              <div className="flex items-center gap-1.5">
                <Icon icon="print" size={11} color="var(--color-text-muted)" />
                <span className="text-xs font-mono">{printCount}</span>
                <span className="text-xs opacity-60">print signal{printCount !== 1 ? 's' : ''}</span>
              </div>
              <div className="flex items-center gap-1.5">
                <Icon icon="usb" size={11} color="var(--color-text-muted)" />
                <span className="text-xs font-mono">{usbCount}</span>
                <span className="text-xs opacity-60">USB signal{usbCount !== 1 ? 's' : ''}</span>
              </div>
            </div>
          )}

          {recent.length > 0 && (
            <ArgusSignalTable entries={recent} title="Recent print and USB signals" formatClass={classLabel} formatMagnitude={formatMagnitude} />
          )}

          {recent.length === 0 && isRunning && (
            <div className="text-xs opacity-60">
              No print or USB signals recorded yet — data appears after the first collection interval.
            </div>
          )}

        </div>
      )}
    </SectionCard>
  );
}
