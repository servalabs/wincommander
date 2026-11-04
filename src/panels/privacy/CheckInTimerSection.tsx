// src/panels/privacy/CheckInTimerSection.tsx
//
// Local app-owned inactivity timer. Startup resets its baseline.

import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button, Icon } from '@/components/ui/bp';
import SectionCard from '../../components/shared/SectionCard';
import { inactivityPresentation } from './inactivityPresentation';
import { createInactivityStatusReader } from './inactivityStatusReader';

const readInactivityStatus = createInactivityStatusReader(() => invoke<unknown>('get_dead_mans_switch_status'));

interface DeadMansSwitchConfig {
  enabled: boolean;
  thresholdDays: number;
  flowIdToFire: string | null;
  lastActivityAt: string;
  lastFiredAt: string | null;
}

interface DeadMansSwitchStatus {
  config: DeadMansSwitchConfig;
  state: unknown;
  startupResetFailed: boolean;
  lastIterationCompletedAt: string | null;
}

function isTimerStatus(value: unknown): value is DeadMansSwitchStatus {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
  const result = value as Record<string, unknown>;
  if (!result.config || typeof result.config !== 'object' || Array.isArray(result.config)) return false;
  const config = result.config as Record<string, unknown>;
  return typeof config.enabled === 'boolean' && typeof config.thresholdDays === 'number' &&
    Number.isFinite(config.thresholdDays) && typeof config.lastActivityAt === 'string' &&
    (config.lastFiredAt === null || typeof config.lastFiredAt === 'string') &&
    (config.flowIdToFire === null || typeof config.flowIdToFire === 'string') &&
    typeof result.state === 'string' && typeof result.startupResetFailed === 'boolean' &&
    (result.lastIterationCompletedAt === null || typeof result.lastIterationCompletedAt === 'string');
}

function daysSince(iso: string): number | null {
  if (!iso) return null;
  try {
    const t = new Date(iso).getTime();
    if (!isFinite(t)) return null;
    return Math.floor((Date.now() - t) / 86400000);
  } catch {
    return null;
  }
}

function formatRelative(iso: string): string {
  const d = daysSince(iso);
  if (d === null) return '—';
  if (d < 0) return 'activity time is ahead of the device clock';
  if (d === 0) return 'today';
  if (d === 1) return '1 day ago';
  return `${d} days ago`;
}

export default function CheckInTimerSection({ bare = false }: { bare?: boolean } = {}) {
  const [config, setConfig] = useState<DeadMansSwitchConfig | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [state, setState] = useState<unknown>(null);
  const [startupResetFailed, setStartupResetFailed] = useState(false);
  const [lastIterationCompletedAt, setLastIterationCompletedAt] = useState<string | null>(null);
  const sequence = useRef(0);
  const savingRef = useRef(false);

  const refresh = useCallback(async () => {
    const current = ++sequence.current;
    try {
      const result = await readInactivityStatus();
      if (current !== sequence.current) return;
      if (!isTimerStatus(result)) throw new Error('Invalid local timer status');
      setConfig(result.config);
      setState(result.state);
      setStartupResetFailed(result.startupResetFailed === true);
      setLastIterationCompletedAt(result.lastIterationCompletedAt);
      setError(null);
    } catch (err) {
      if (current !== sequence.current) return;
      setState(null);
      setStartupResetFailed(false);
      setLastIterationCompletedAt(null);
      setError(String(err));
    }
  }, []);

  useEffect(() => {
    const activeSequence = sequence;
    void refresh();
    const interval = setInterval(() => { if (!savingRef.current) void refresh(); }, 30_000);
    return () => { clearInterval(interval); activeSequence.current++; };
  }, [refresh]);

  const saveConfig = useCallback(
    async (patch: Partial<DeadMansSwitchConfig>) => {
      if (!config || savingRef.current) return;
      savingRef.current = true;
      sequence.current++;
      setState(null);
      setSaving(true);
      setError(null);
      try {
        const next = { ...config, ...patch };
        const saved = await invoke<DeadMansSwitchConfig>('set_dead_mans_switch_config', {
          config: next,
        });
        setConfig(saved);
        await refresh();
      } catch (err) {
        setError(String(err));
      } finally {
        savingRef.current = false;
        setSaving(false);
      }
    },
    [config, refresh],
  );

  const handleReset = useCallback(async () => {
    if (savingRef.current) return;
    savingRef.current = true;
    sequence.current++;
    setState(null);
    setSaving(true);
    setError(null);
    try {
      const c = await invoke<DeadMansSwitchConfig>('reset_dead_mans_switch_timer');
      setConfig(c);
      await refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  }, [refresh]);

  const handleClearFired = useCallback(async () => {
    if (savingRef.current) return;
    savingRef.current = true;
    sequence.current++;
    setState(null);
    setSaving(true);
    setError(null);
    try {
      const c = await invoke<DeadMansSwitchConfig>('clear_dead_mans_switch_fired');
      setConfig(c);
      await refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  }, [refresh]);

  if (!config) {
    const message = <p role={error ? 'alert' : 'status'}>{error ? 'Inactivity timer status could not be confirmed.' : 'Loading inactivity timer…'}</p>;
    return bare ? <div className="lockdown-trigger-block">{message}</div> : <SectionCard title="Inactivity Check-In" icon="time">{message}</SectionCard>;
  }

  const daysIdle = daysSince(config.lastActivityAt);
  const daysRemaining =
    daysIdle === null || daysIdle < 0 ? null : Math.max(0, config.thresholdDays - daysIdle);

  const status = inactivityPresentation(state);
  const statusTag = <span className="lockdown-trigger-status">{status.label}</span>;

  const inner = (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      {config.lastFiredAt && (
        <div style={{ padding: '8px 10px', background: 'var(--color-danger-dim)', border: '1px solid color-mix(in srgb, var(--color-danger) 50%, transparent)', borderRadius: 4 }}>
          <div style={{ fontWeight: 700, color: 'var(--color-danger)', fontSize: 11 }}>
            Trigger recorded · {new Date(config.lastFiredAt).toLocaleDateString()}
          </div>
          <Button intent="danger" small icon="reset" onClick={handleClearFired} disabled={saving} style={{ marginTop: 6 }}>
            Rearm
          </Button>
        </div>
      )}

      <p role="status">{status.message}</p>
      {lastIterationCompletedAt && Number.isFinite(new Date(lastIterationCompletedAt).getTime()) && <p>Last completed iteration: <time dateTime={lastIterationCompletedAt}>{new Date(lastIterationCompletedAt).toLocaleString()}</time> (historical).</p>}
      {startupResetFailed && <p role="alert">The timer could not reset when WinCommander started. Check the activity time; the watchdog may still run.</p>}
      <p>The timer runs only while WinCommander and personal automation are available. Starting WinCommander resets the activity time, including automatic startup; it does not prove that a person checked in.</p>

      <div className="lockdown-trigger-row">
        <input
          type="number"
          min={1}
          max={365}
          value={config.thresholdDays}
          onChange={(e) => {
            const v = Number(e.target.value);
            if (Number.isFinite(v) && v >= 1 && v <= 365) saveConfig({ thresholdDays: Math.round(v) });
          }}
          disabled={!config.enabled || saving}
          className="lockdown-trigger-input"
          style={{ width: 64 }}
        />
        <span>days since activity → trigger attempt</span>
      </div>

      <div className="lockdown-trigger-row">
        <span style={{ color: 'var(--color-text-secondary)', fontSize: 11 }}>→ requests the configured Lockdown when enabled</span>
      </div>

      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8, padding: '6px 8px', background: 'var(--color-bg-tertiary)', borderRadius: 4 }}>
        <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--color-text-secondary)' }}>
          {formatRelative(config.lastActivityAt)}
          {daysRemaining !== null && config.enabled && (
            <span style={{ color: 'var(--color-text-muted)', marginLeft: 6 }}>· {daysRemaining}d left</span>
          )}
        </span>
        <Button intent="primary" icon="hand" onClick={handleReset} disabled={saving} small>I'm alive</Button>
      </div>

      {error && (
        <div role="alert" style={{ padding: 8, background: 'var(--color-danger-dim)', border: '1px solid color-mix(in srgb, var(--color-danger) 50%, transparent)', borderRadius: 4, fontSize: 11, color: 'var(--color-danger)' }}>
          {error}
        </div>
      )}
    </div>
  );

  if (bare) {
    return (
      <div className="lockdown-trigger-block">
        <div className="lockdown-trigger-head">
          <Icon icon="time" size={14} className="lockdown-trigger-icon" />
          <span className="lockdown-trigger-title">Inactivity Check-In</span>
          {statusTag}
          <button
            type="button"
            role="switch"
            aria-checked={config.enabled}
            className={`lockdown-trigger-toggle ${config.enabled ? 'is-on' : ''}`}
            onClick={() => saveConfig({ enabled: !config.enabled })}
            disabled={saving}
          />
        </div>
        <p className="lockdown-trigger-desc">
          Requests Lockdown after the configured inactivity period while the app is running. Click "I'm alive" to reset the timer.
        </p>
        {inner}
      </div>
    );
  }
  return (
    <SectionCard title="Inactivity Check-In" icon="time" headerRight={statusTag}>
      {inner}
    </SectionCard>
  );
}
