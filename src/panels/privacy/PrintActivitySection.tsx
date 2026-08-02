// src/panels/privacy/PrintActivitySection.tsx
//
// "Print Audit" — surface Windows print-job history so the operator
// can catch paper-based exfil. The actual recording is done by the
// Microsoft-Windows-PrintService/Operational event channel (which
// ships disabled); we expose a one-click toggle to flip it on plus a
// read-out of recent Event 307 entries.
//
// The channel is the durable store — we never lose events to UI being
// closed, panel polling cadence, etc. The frontend is purely a read
// surface.

import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button, Spinner, Switch, Tag } from '@/components/ui/bp';
import SectionCard from '../../components/shared/SectionCard';

interface PrintAuditEntry {
  timeCreated: string;
  document: string;
  pages: number;
  printer: string;
  user: string;
}

interface PrintAuditStatus {
  channelEnabled: boolean;
  channelPresent: boolean;
}

function formatTime(iso: string): string {
  if (!iso) return '—';
  try {
    const d = new Date(iso);
    return d.toLocaleString(undefined, {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  } catch {
    return iso;
  }
}

function basename(path: string): string {
  if (!path) return '—';
  return path.split(/[/\\]/).pop() ?? path;
}

export default function PrintActivitySection() {
  const [status, setStatus] = useState<PrintAuditStatus | null>(null);
  const [entries, setEntries] = useState<PrintAuditEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [toggling, setToggling] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const s = await invoke<PrintAuditStatus>('get_print_audit_status');
      setStatus(s);
      if (s.channelEnabled) {
        const log = await invoke<PrintAuditEntry[]>('get_print_audit_log', { limit: 50 });
        setEntries(log);
      } else {
        setEntries([]);
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const handleToggle = useCallback(
    async (next: boolean) => {
      setToggling(true);
      setError(null);
      try {
        await invoke('set_print_audit_enabled', { enabled: next });
        await refresh();
      } catch (err) {
        setError(String(err));
      } finally {
        setToggling(false);
      }
    },
    [refresh],
  );

  const headerTag = status?.channelEnabled ? (
    <Tag minimal intent="success" style={{ fontSize: 9 }}>RECORDING</Tag>
  ) : null;

  return (
    <SectionCard title="Print Audit" icon="print" headerRight={headerTag}>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
        <div style={{ fontSize: 12, color: 'var(--color-text-secondary)', lineHeight: 1.5 }}>
          Catches paper-based exfil — the #1 insider-leak vector in
          regulated industries. Reads from the Windows{' '}
          <code>Microsoft-Windows-PrintService/Operational</code> channel
          (Event 307). Enabling requires admin once; Windows handles the
          recording from there.
        </div>

        <div style={{ display: 'flex', gap: 12, alignItems: 'center', flexWrap: 'wrap' }}>
          <Switch
            checked={!!status?.channelEnabled}
            onChange={(e) => handleToggle((e.target as HTMLInputElement).checked)}
            disabled={!status?.channelPresent || toggling}
            label={
              status?.channelEnabled
                ? 'Recording print jobs'
                : status?.channelPresent
                  ? 'Enable print audit (admin)'
                  : 'Print service unavailable on this Windows build'
            }
            large
            style={{ marginBottom: 0 }}
          />
          {toggling && <Spinner size={14} />}
          <Button icon="refresh" minimal small onClick={refresh} disabled={loading} aria-label="Refresh print audit events">
            Refresh
          </Button>
        </div>

        {error && (
          <div
            style={{
              padding: 8,
              background: 'var(--color-danger-dim)',
              border: '1px solid color-mix(in srgb, var(--color-danger) 50%, transparent)',
              borderRadius: 4,
              fontSize: 11,
              color: 'var(--color-danger)',
            }}
          >
            {error}
          </div>
        )}

        {status?.channelEnabled && (
          <div>
            <div
              style={{
                fontSize: 11,
                fontWeight: 700,
                textTransform: 'uppercase',
                letterSpacing: 0.6,
                marginBottom: 8,
                color: 'var(--color-text-muted)',
              }}
            >
              Recent print jobs ({entries.length})
            </div>
            {loading && (
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 12 }}>
                <Spinner size={14} /> Reading…
              </div>
            )}
            {!loading && entries.length === 0 && (
              <div style={{ fontSize: 12, color: 'var(--color-text-muted)' }}>
                No print jobs recorded yet. Print something to verify.
              </div>
            )}
            {!loading && entries.length > 0 && (
              <div
                style={{
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 4,
                  maxHeight: 320,
                  overflowY: 'auto',
                }}
              >
                {entries.map((e, i) => (
                  <div
                    key={`${e.timeCreated}-${i}`}
                    style={{
                      padding: '6px 10px',
                      background: 'var(--color-bg-secondary)',
                      border: '1px solid var(--color-border)',
                      borderRadius: 4,
                      fontSize: 11,
                    }}
                  >
                    <div style={{ display: 'flex', gap: 10, alignItems: 'center', flexWrap: 'wrap' }}>
                      <span
                        style={{
                          fontFamily: 'var(--font-mono)',
                          fontSize: 10,
                          color: 'var(--color-text-muted)',
                          minWidth: 130,
                        }}
                      >
                        {formatTime(e.timeCreated)}
                      </span>
                      <span style={{ fontWeight: 600, flex: 1, minWidth: 120 }}>
                        {basename(e.document)}
                      </span>
                      <Tag minimal style={{ fontSize: 10 }}>
                        {e.pages} page{e.pages === 1 ? '' : 's'}
                      </Tag>
                    </div>
                    <div
                      style={{
                        marginTop: 3,
                        fontSize: 10,
                        color: 'var(--color-text-muted)',
                      }}
                    >
                      via <strong>{e.printer || '—'}</strong> as{' '}
                      <strong>{e.user || '—'}</strong>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </SectionCard>
  );
}
