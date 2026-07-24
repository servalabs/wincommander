// src/panels/privacy/CheckInTimerSection.tsx
//
// "Dead Man's Switch" — triggers the Lockdown / self-destruct cascade if
// WinCommander doesn't see the operator for N days. Useful for
// journalists/activists/investigators: if the operator disappears
// (arrest, seizure, …) the device destroys itself rather than sitting
// accessible.

import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button, Icon } from '@/components/ui/bp';
import SectionCard from '../../components/shared/SectionCard';

interface DeadMansSwitchConfig {
  enabled: boolean;
  thresholdDays: number;
  flowIdToFire: string | null;
  lastActivityAt: string;
  lastFiredAt: string | null;
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
  if (d === 0) return 'today';
  if (d === 1) return '1 day ago';
  return `${d} days ago`;
}

export default function CheckInTimerSection({ bare = false }: { bare?: boolean } = {}) {
  const [config, setConfig] = useState<DeadMansSwitchConfig | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setError(null);
    try {
      const c = await invoke<DeadMansSwitchConfig>('get_dead_mans_switch_config');
      setConfig(c);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const saveConfig = useCallback(
    async (patch: Partial<DeadMansSwitchConfig>) => {
      if (!config) return;
      setSaving(true);
      setError(null);
      try {
        const next = { ...config, ...patch };
        const saved = await invoke<DeadMansSwitchConfig>('set_dead_mans_switch_config', {
          config: next,
        });
        setConfig(saved);
      } catch (err) {
        setError(String(err));
      } finally {
        setSaving(false);
      }
    },
    [config],
  );

  const handleReset = useCallback(async () => {
    setSaving(true);
    setError(null);
    try {
      const c = await invoke<DeadMansSwitchConfig>('reset_dead_mans_switch_timer');
      setConfig(c);
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }, []);

  const handleClearFired = useCallback(async () => {
    setSaving(true);
    setError(null);
    try {
      const c = await invoke<DeadMansSwitchConfig>('clear_dead_mans_switch_fired');
      setConfig(c);
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }, []);

  if (!config) {
    return null;
  }

  const daysIdle = daysSince(config.lastActivityAt);
  const daysRemaining =
    daysIdle === null ? null : Math.max(0, config.thresholdDays - daysIdle);

  const statusTag = config.lastFiredAt ? (
    <span className="lockdown-trigger-status" style={{ background: 'var(--color-danger-dim)', color: 'var(--color-danger)' }}>Tripped</span>
  ) : config.enabled ? (
    <span className="lockdown-trigger-status">Active</span>
  ) : null;

  const inner = (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      {config.lastFiredAt && (
        <div style={{ padding: '8px 10px', background: 'var(--color-danger-dim)', border: '1px solid color-mix(in srgb, var(--color-danger) 50%, transparent)', borderRadius: 4 }}>
          <div style={{ fontWeight: 700, color: 'var(--color-danger)', fontSize: 11 }}>
            Tripped · {new Date(config.lastFiredAt).toLocaleDateString()}
          </div>
          <Button intent="danger" small icon="reset" onClick={handleClearFired} disabled={saving} style={{ marginTop: 6 }}>
            Rearm
          </Button>
        </div>
      )}

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
        <span>days idle → fire</span>
      </div>

      <div className="lockdown-trigger-row">
        <span style={{ color: 'var(--color-text-secondary)', fontSize: 11 }}>→ fires Lockdown (self-destruct cascade)</span>
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
        <div style={{ padding: 8, background: 'var(--color-danger-dim)', border: '1px solid color-mix(in srgb, var(--color-danger) 50%, transparent)', borderRadius: 4, fontSize: 11, color: 'var(--color-danger)' }}>
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
          Triggers the Lockdown cascade if there's no activity for N days. Click "I'm alive" to reset the timer.
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
