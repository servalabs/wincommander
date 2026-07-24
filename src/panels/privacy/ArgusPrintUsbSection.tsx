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

function severityColor(sev: string): string {
  if (sev === 'critical' || sev === 'high') return 'var(--color-danger, #f87171)';
  if (sev === 'warn' || sev === 'medium') return 'var(--color-warning, #fbbf24)';
  return 'var(--color-text-muted)';
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

function kindIcon(kind: string): 'print' | 'usb' | 'dot' {
  if (kind === 'print') return 'print';
  if (kind === 'removable_media') return 'usb';
  return 'dot';
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
  if (isRunning) {
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
              disabled={monitorBusy}
              onChange={(e) => void toggleCollector((e.target as HTMLInputElement).checked)}
              label="Print &amp; USB monitoring active"
            />
            <Button
              icon="refresh"
              minimal
              small
              disabled={monitorBusy}
              onClick={() => void refreshStatus()}
            >
              Refresh
            </Button>
            {monitorBusy && <Spinner size={14} />}
          </div>

          {monitorError && (
            <div className="font-mono text-xs text-[var(--color-danger,#f87171)]">{monitorError}</div>
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

          {/* Recent signals — aggregate only (no names, no paths) */}
          {recent.length > 0 && (
            <div className="flex flex-col gap-2">
              <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
                Recent signals ({recent.length})
              </span>
              <div className="flex flex-col gap-1.5 max-h-[260px] overflow-y-auto pr-1">
                {[...recent].reverse().map((entry, i) => (
                  <div
                    key={`${entry.windowStart}-${entry.class}-${i}`}
                    className="rounded border px-3 py-2 flex flex-col gap-1"
                    style={{
                      background: 'var(--color-bg-secondary)',
                      borderColor: `${severityColor(entry.severity)}40`,
                    }}
                  >
                    <div className="flex items-center justify-between gap-2 flex-wrap">
                      <div className="flex items-center gap-2">
                        <span
                          className="text-[10px] px-1.5 py-0.5 rounded border font-mono flex-shrink-0"
                          style={{
                            color: severityColor(entry.severity),
                            borderColor: `${severityColor(entry.severity)}50`,
                            background: `${severityColor(entry.severity)}15`,
                          }}
                        >
                          {entry.severity.toUpperCase()}
                        </span>
                        <span
                          className="text-[10px] px-1.5 py-0.5 rounded border font-mono"
                          style={{
                            color: 'var(--color-accent)',
                            borderColor: 'var(--color-accent-dim)',
                            background: 'var(--color-accent-subtle)',
                          }}
                        >
                          {classLabel(entry.class)}
                        </span>
                      </div>
                      <span className="text-[10px] font-mono opacity-50 flex-shrink-0">
                        {entry.windowStart.slice(11, 16)} – {entry.windowEnd.slice(11, 16)}
                      </span>
                    </div>
                    <div className="flex items-center gap-2">
                      <span className="text-[10px] font-mono text-[var(--shield-text-muted)] opacity-70">
                        {kindIcon(entry.kind) === 'print' ? 'pages:' : 'devices:'}
                      </span>
                      <span
                        className="text-[10px] font-mono font-semibold"
                        style={{ color: severityColor(entry.severity) }}
                      >
                        {formatMagnitude(entry)}
                      </span>
                    </div>
                  </div>
                ))}
              </div>
            </div>
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
