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

function sevIntent(sev: string): Intent | undefined {
  return sev === 'high' ? 'danger' : undefined;
}

export default function AuthAnomalySection() {
  const requestConfirm = useAppConfirm();
  const { canUse } = useEntitlements();
  const [running, setRunning] = useState(false);
  const [hits, setHits] = useState<AuthAnomalyHit[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setError(null);
    try {
      const status = await invoke<{ running: boolean }>('auth_anomaly_status');
      setRunning(!!status?.running);
      const r = await invoke<AuthAnomalyHit[]>('get_auth_anomaly_recent');
      setHits(Array.isArray(r) ? r : []);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const toggle = useCallback(
    async (on: boolean) => {
      setBusy(true);
      setError(null);
      try {
        await invoke(on ? 'start_auth_anomaly_monitor' : 'stop_auth_anomaly_monitor');
        await refresh();
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(false);
      }
    },
    [refresh],
  );

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
            checked={running}
            disabled={busy}
            onChange={(e) => toggle((e.target as HTMLInputElement).checked)}
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

        {error && <div className="font-mono text-sm text-[var(--color-danger)]">{error}</div>}
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
