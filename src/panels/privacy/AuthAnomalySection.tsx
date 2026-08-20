// src/panels/privacy/AuthAnomalySection.tsx
//
// "Access & Session Monitor" — surfaces Windows sign-in anomalies from the
// Security log (failed-logon bursts, new local accounts, remote/RDP sign-ins,
// off-hours logons). The detector runs in the Pro sidecar (PAID); this is the
// thin UI. Sits beside the Remote-Access tripwire to form the access-session
// surface (the owner's "access-session" merge of login-anomaly + remote-access).

import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button, Spinner, Switch, Tag } from '@/components/ui/bp';
import type { Intent } from '@/components/ui/bp';
import type { AuthAnomalyTimeBasis } from '@/types/settings';
import useEntitlements from '@/hooks/useEntitlements';
import SectionCard from '../../components/shared/SectionCard';
import { useAppConfirm } from '../../components/shared/AppConfirmDialog';

interface AuthAnomalyHit {
  kind: string;
  severity: 'info' | 'high';
  summary: string;
  user: string | null;
  ip: string | null;
  count: number | null;
  detectedAt: string;
}

interface AuthAnomalyPolicyProps {
  enabled: boolean;
  failedBurstThreshold: number;
  failedBurstWindowSecs: number;
  workStartHour: number;
  workEndHour: number;
  workDays: number[];
  timeBasis: AuthAnomalyTimeBasis;
  detectRdp: boolean;
  detectNewAccounts: boolean;
  detectOffHours: boolean;
  alertDebounceSecs: number;
  reportToFleet: boolean;
  fleetReportingRequired: boolean;
  onPatch: (patch: AuthAnomalyPolicyPatch) => void;
}

type AuthAnomalyPolicyPatch = Partial<Omit<AuthAnomalyPolicyProps, 'onPatch' | 'fleetReportingRequired'>>;

function sevIntent(sev: string): Intent | undefined {
  return sev === 'high' ? 'danger' : undefined;
}

export default function AuthAnomalySection({
  enabled,
  failedBurstThreshold,
  failedBurstWindowSecs,
  workStartHour,
  workEndHour,
  workDays,
  timeBasis,
  detectRdp,
  detectNewAccounts,
  detectOffHours,
  alertDebounceSecs,
  reportToFleet,
  fleetReportingRequired,
  onPatch,
}: AuthAnomalyPolicyProps) {
  const requestConfirm = useAppConfirm();
  const { canUse } = useEntitlements();
  const [running, setRunning] = useState(false);
  const [hits, setHits] = useState<AuthAnomalyHit[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setError(null);
    try {
      const status = await invoke<{
        running: boolean;
        collectorHealthy?: boolean;
        lastError?: string | null;
      }>('auth_anomaly_status');
      setRunning(!!status?.running);
      if (status?.collectorHealthy === false) {
        setError(status.lastError || 'Unable to read the Windows Security log.');
      }
      const r = await invoke<AuthAnomalyHit[]>('get_auth_anomaly_recent');
      setHits(Array.isArray(r) ? r : []);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [enabled, refresh]);

  useEffect(() => {
    const onHealth = (event: Event) => {
      const detail = (event as CustomEvent<{ error?: string | null }>).detail;
      if (detail) setError(detail.error ?? null);
    };
    window.addEventListener('auth-anomaly-monitor-health', onHealth);
    return () => window.removeEventListener('auth-anomaly-monitor-health', onHealth);
  }, []);

  const clear = useCallback(async () => {
    const accepted = await requestConfirm({
      title: 'Clear recent access anomalies?',
      description: 'This removes the recent sign-in anomaly findings recorded for this app session.',
      confirmLabel: 'Clear findings',
    });
    if (!accepted) return;
    setBusy(true);
    try {
      await invoke('clear_auth_anomaly_recent');
      setHits([]);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [requestConfirm]);

  if (!canUse('paid')) {
    return (
      <SectionCard title="Access & Session Monitor" icon="shield" headerRight={<Tag minimal intent="none" className="font-mono">PRO</Tag>}>
        <p className="text-sm opacity-70">
          Watches the Windows Security log for sign-in anomalies — failed-logon bursts, new local
          accounts, remote (RDP) sign-ins, and off-hours logons. Requires Pro.
        </p>
      </SectionCard>
    );
  }

  const highCount = hits.filter((h) => h.severity === 'high').length;
  const headerRight = (
    <Tag minimal intent={highCount > 0 ? 'danger' : running ? 'success' : 'none'} className="font-mono">
      {highCount > 0
        ? `${highCount} ALERT${highCount === 1 ? '' : 'S'}`
        : running
          ? 'WATCHING'
          : 'OFF'}
    </Tag>
  );

  return (
    <SectionCard title="Access & Session Monitor" icon="shield" headerRight={headerRight}>
      <div className="flex flex-col gap-3">
        <div className="text-sm opacity-80">
          Watches the Windows Security log for sign-in anomalies — failed-logon bursts, new local
          accounts, remote (RDP) sign-ins, and off-hours logons.
        </div>
        <p className="text-[10px] opacity-70">
          When fleet monitoring is active, these sign-in
          anomalies are also reported to the fleet as aggregate "access" signals — anomaly type
          and count only, never usernames or IP addresses.
        </p>

        <div className="flex items-center gap-3">
          <Switch
            checked={enabled}
            disabled={busy}
            onChange={(e) => onPatch({ enabled: e.currentTarget.checked })}
            label="Monitor sign-in activity"
          />
          <Button icon="refresh" minimal small onClick={() => void refresh()} disabled={busy}>
            Refresh
          </Button>
          <Button icon="trash" minimal small onClick={clear} disabled={busy || hits.length === 0}>
            Clear
          </Button>
          {busy && <Spinner size={14} />}
        </div>

        {enabled && (
          <div className="grid gap-3 border-t border-white/10 pt-3 sm:grid-cols-2">
            <label className="flex flex-col gap-1 text-xs">
              Failed sign-ins before alert
              <input
                className="rounded border border-white/15 bg-transparent px-2 py-1 font-mono"
                type="number"
                min={2}
                max={50}
                value={failedBurstThreshold}
                onChange={(e) => onPatch({ failedBurstThreshold: Math.max(2, Math.min(50, Number(e.currentTarget.value))) })}
              />
            </label>
            <label className="flex flex-col gap-1 text-xs">
              Failed sign-in window (seconds)
              <input
                className="rounded border border-white/15 bg-transparent px-2 py-1 font-mono"
                type="number"
                min={30}
                max={900}
                step={30}
                value={failedBurstWindowSecs}
                onChange={(e) => onPatch({ failedBurstWindowSecs: Math.max(30, Math.min(900, Number(e.currentTarget.value))) })}
              />
            </label>
            <label className="flex flex-col gap-1 text-xs">
              Working hours start
              <input
                className="rounded border border-white/15 bg-transparent px-2 py-1 font-mono"
                type="number"
                min={0}
                max={23}
                value={workStartHour}
                onChange={(e) => {
                  const next = Math.max(0, Math.min(23, Number(e.currentTarget.value)));
                  onPatch({ workStartHour: next, workEndHour: next === workEndHour ? (next + 1) % 24 : workEndHour });
                }}
              />
            </label>
            <label className="flex flex-col gap-1 text-xs">
              Working hours end
              <input
                className="rounded border border-white/15 bg-transparent px-2 py-1 font-mono"
                type="number"
                min={0}
                max={23}
                value={workEndHour}
                onChange={(e) => {
                  const next = Math.max(0, Math.min(23, Number(e.currentTarget.value)));
                  onPatch({ workEndHour: next, workStartHour: next === workStartHour ? (next + 23) % 24 : workStartHour });
                }}
              />
            </label>
            <label className="flex flex-col gap-1 text-xs">
              Schedule time basis
              <select
                className="rounded border border-white/15 bg-transparent px-2 py-1"
                value={timeBasis}
                onChange={(e) => onPatch({ timeBasis: e.currentTarget.value as AuthAnomalyTimeBasis })}
              >
                <option value="local">This device's local time</option>
                <option value="utc">UTC</option>
              </select>
            </label>
            <div className="flex flex-col gap-1 text-xs sm:col-span-2">
              <span>Working days</span>
              <div className="flex flex-wrap gap-x-3 gap-y-1">
                {[
                  [1, 'Mon'], [2, 'Tue'], [3, 'Wed'], [4, 'Thu'], [5, 'Fri'], [6, 'Sat'], [7, 'Sun'],
                ].map(([day, label]) => {
                  const dayNumber = day as number;
                  const selected = workDays.includes(dayNumber);
                  return (
                    <label key={dayNumber} className="flex items-center gap-1">
                      <input
                        type="checkbox"
                        checked={selected}
                        disabled={selected && workDays.length === 1}
                        onChange={(e) => {
                          const next = e.currentTarget.checked
                            ? [...new Set([...workDays, dayNumber])].sort((a, b) => a - b)
                            : workDays.filter((configuredDay) => configuredDay !== dayNumber);
                          if (next.length > 0) onPatch({ workDays: next });
                        }}
                      />
                      {label}
                    </label>
                  );
                })}
              </div>
            </div>
            <label className="flex flex-col gap-1 text-xs">
              Repeat-alert delay (seconds)
              <input
                className="rounded border border-white/15 bg-transparent px-2 py-1 font-mono"
                type="number"
                min={30}
                max={3600}
                step={30}
                value={alertDebounceSecs}
                onChange={(e) => onPatch({ alertDebounceSecs: Math.max(30, Math.min(3600, Number(e.currentTarget.value))) })}
              />
            </label>
            <div className="flex flex-col gap-2 text-xs sm:col-span-2">
              <Switch checked={detectRdp} onChange={(e) => onPatch({ detectRdp: e.currentTarget.checked })} label="Alert on RDP / remote sign-ins" />
              <Switch checked={detectNewAccounts} onChange={(e) => onPatch({ detectNewAccounts: e.currentTarget.checked })} label="Alert when a local account is created" />
              <Switch checked={detectOffHours} onChange={(e) => onPatch({ detectOffHours: e.currentTarget.checked })} label="Alert on interactive sign-ins outside the schedule" />
              <Switch
                checked={fleetReportingRequired || reportToFleet}
                disabled={fleetReportingRequired}
                onChange={(e) => onPatch({ reportToFleet: e.currentTarget.checked })}
                label={fleetReportingRequired
                  ? 'Fleet reporting required by device policy'
                  : 'Send aggregate access signals to Fleet'}
              />
              <p className="opacity-70">An overnight schedule is supported: start it late in the day and end it the following morning.</p>
            </div>
          </div>
        )}

        {error && <div role="alert" className="font-mono text-sm text-[var(--color-danger)]">{error}</div>}
        {hits.length === 0 && <div className="text-sm opacity-70">No anomalies recorded.</div>}

        {hits.length > 0 && (
          <div className="flex flex-col gap-2">
            {hits
              .slice()
              .reverse()
              .map((h, i) => (
                <div
                  key={`${h.kind}-${h.detectedAt}-${i}`}
                  className="flex items-start gap-2 border-t border-white/10 pt-2"
                >
                  <Tag minimal={h.severity === 'info'} intent={sevIntent(h.severity)} className="font-mono">
                    {h.severity.toUpperCase()}
                  </Tag>
                  <div className="flex-1">
                    <div className="text-sm">{h.summary}</div>
                    <div className="font-mono text-xs opacity-60">{h.detectedAt}</div>
                  </div>
                </div>
              ))}
          </div>
        )}
      </div>
    </SectionCard>
  );
}
