// src/panels/network/WifiGuardSection.tsx
//
// Wi-Fi Guard — paid feature that polls `netsh wlan show
// interfaces`, learns SSID→BSSID associations passively for 24h after
// arming, then fires when the connected SSID is known but the BSSID
// is unfamiliar (rogue AP impersonation) or auth strength downgrades.
//
// UI surfaces: arm/disarm, learning-mode indicator, list of known
// SSIDs (with collapsible BSSIDs), recent fires, and a forget-BSSID
// action.

import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { Button, Icon, InputGroup, Spinner, Switch, Tag } from '@/components/ui/bp';
import SectionCard from '../../components/shared/SectionCard';
import { useAppConfirm } from '../../components/shared/AppConfirmDialog';
import { useAppState } from '../../context/AppContext';
import { isPrivilegedWriteBlocked, MACHINE_SCOPE_ELEVATION_MESSAGE } from '../../lib/machineScopeElevation';
import { reportSettingsWriteFailure } from '../../lib/settingsWriteRecovery';
import {
  DEFAULT_WIFI_GUARD_ALERT_DEBOUNCE_SECS,
  DEFAULT_WIFI_GUARD_LEARNING_WINDOW_SECS,
  DEFAULT_WIFI_GUARD_POLL_INTERVAL_SECS,
} from '../../hooks/useWifiGuardMonitor';
import type { WifiGuardBaselineEntry } from '../../types/settings';

/** Local callout replacement — our global `.bp6-callout` overrides
 *  fight BP's default `::before` icon (absolute-positioned) so icons
 *  end up overlapping the title. This is a controlled grid-based
 *  alternative that guarantees the icon, title, and body live in their
 *  own cells. */
function IntelNotice({
  intent,
  icon,
  title,
  children,
}: {
  intent: 'warning' | 'danger' | 'info' | 'success';
  icon: React.ComponentProps<typeof Icon>['icon'];
  title: string;
  children: React.ReactNode;
}) {
  const palette = {
    warning: {
      border: 'color-mix(in srgb, var(--color-warning) 45%, transparent)',
      bg: 'color-mix(in srgb, var(--color-warning) 12%, transparent)',
      color: 'var(--color-warning)',
    },
    danger: {
      border: 'color-mix(in srgb, var(--color-danger) 45%, transparent)',
      bg: 'color-mix(in srgb, var(--color-danger) 12%, transparent)',
      color: 'var(--color-danger)',
    },
    info: {
      border: 'color-mix(in srgb, var(--color-info) 45%, transparent)',
      bg: 'color-mix(in srgb, var(--color-info) 12%, transparent)',
      color: 'var(--color-info)',
    },
    success: {
      border: 'color-mix(in srgb, var(--color-success) 45%, transparent)',
      bg: 'color-mix(in srgb, var(--color-success) 12%, transparent)',
      color: 'var(--color-success)',
    },
  }[intent];

  return (
    <div
      style={{
        display: 'grid',
        gridTemplateColumns: '20px 1fr',
        gap: 10,
        alignItems: 'start',
        padding: '10px 12px',
        background: palette.bg,
        border: `1px solid ${palette.border}`,
        borderRadius: 4,
      }}
    >
      <Icon icon={icon} size={16} color={palette.color} style={{ marginTop: 1 }} />
      <div style={{ minWidth: 0, display: 'flex', flexDirection: 'column', gap: 2 }}>
        <div
          style={{
            fontSize: 12,
            fontWeight: 700,
            color: palette.color,
            lineHeight: 1.3,
          }}
        >
          {title}
        </div>
        <div style={{ fontSize: 11, lineHeight: 1.45, color: 'var(--color-text-secondary)' }}>
          {children}
        </div>
      </div>
    </div>
  );
}

interface WifiGuardStatus {
  running: boolean;
  learning: boolean;
  knownSsidCount: number;
  currentSsid: string | null;
  currentBssid: string | null;
  learningUntil: string | null;
  learningWindowSecs: number;
  pollIntervalSecs: number;
  alertDebounceSecs: number;
}

interface WifiGuardHit {
  ssid: string;
  bssid: string;
  auth: string;
  signal: string;
  reason: string;
  detectedAt: string;
}

type KnownEntry = [string, string[]]; // [ssid, bssids]

function formatTime(iso: string): string {
  if (!iso) return '—';
  try {
    return new Date(iso).toLocaleString(undefined, {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  } catch {
    return iso;
  }
}

function formatDuration(seconds: number): string {
  if (seconds % 3600 === 0) return `${seconds / 3600} h`;
  if (seconds >= 60) return `${Math.round(seconds / 60)} min`;
  return `${seconds} s`;
}

function reasonLabel(reason: string): { label: string; intent: 'danger' | 'warning' } {
  switch (reason) {
    case 'both':
      return { label: 'NEW BSSID + AUTH DOWNGRADE', intent: 'danger' };
    case 'newBssid':
      return { label: 'UNKNOWN BSSID', intent: 'danger' };
    case 'authDowngrade':
      return { label: 'AUTH DOWNGRADE', intent: 'warning' };
    default:
      return { label: reason.toUpperCase(), intent: 'warning' };
  }
}

function WifiGuardPolicyControls({
  enabled,
  learningWindowSecs,
  pollIntervalSecs,
  alertDebounceSecs,
  reportToFleet,
  fleetReportingRequired,
  onPatch,
}: {
  enabled: boolean;
  learningWindowSecs: number;
  pollIntervalSecs: number;
  alertDebounceSecs: number;
  reportToFleet: boolean;
  fleetReportingRequired: boolean;
  onPatch: (patch: {
    enabled?: boolean;
    learningWindowSecs?: number;
    pollIntervalSecs?: number;
    alertDebounceSecs?: number;
    learningUntil?: string | null;
    reportToFleet?: boolean;
  }) => void;
}) {
  const numberValue = (value: string, fallback: number, min: number, max: number) => {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? Math.max(min, Math.min(max, Math.round(parsed))) : fallback;
  };
  return (
    <div
      style={{
        display: 'flex', flexDirection: 'column', gap: 8, padding: 10,
        background: 'var(--color-bg-secondary)', border: '1px solid var(--color-border)', borderRadius: 4,
      }}
    >
      <div style={{ fontSize: 10, fontWeight: 700, textTransform: 'uppercase', letterSpacing: 0.6, color: 'var(--color-text-muted)' }}>
        Guard policy
      </div>
      <div style={{ fontSize: 11, color: 'var(--color-text-secondary)', lineHeight: 1.45 }}>
        These values tune observation and repeat alerts; they do not block Wi-Fi or prove that a hotspot is malicious.
      </div>
      <Switch
        checked={enabled}
        onChange={(event) => onPatch({ enabled: (event.target as HTMLInputElement).checked })}
        label="Re-arm automatically after WinCommander starts"
        style={{ marginBottom: 0 }}
      />
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(115px, 1fr))', gap: 8 }}>
        <label style={{ fontSize: 11, color: 'var(--color-text-secondary)' }}>
          Learning (hours)
          <InputGroup
            type="number" min={1 / 12} max={168}
            value={String(learningWindowSecs / 3600)} small
            onChange={(event) => onPatch({ learningWindowSecs: numberValue((event.target as HTMLInputElement).value, learningWindowSecs / 3600, 1 / 12, 168) * 3600 })}
          />
        </label>
        <label style={{ fontSize: 11, color: 'var(--color-text-secondary)' }}>
          Check every (seconds)
          <InputGroup
            type="number" min={5} max={300}
            value={String(pollIntervalSecs)} small
            onChange={(event) => onPatch({ pollIntervalSecs: numberValue((event.target as HTMLInputElement).value, pollIntervalSecs, 5, 300) })}
          />
        </label>
        <label style={{ fontSize: 11, color: 'var(--color-text-secondary)' }}>
          Repeat alert (seconds)
          <InputGroup
            type="number" min={30} max={3600}
            value={String(alertDebounceSecs)} small
            onChange={(event) => onPatch({ alertDebounceSecs: numberValue((event.target as HTMLInputElement).value, alertDebounceSecs, 30, 3600) })}
          />
        </label>
      </div>
      <Switch
        checked={fleetReportingRequired || reportToFleet}
        disabled={fleetReportingRequired}
        onChange={(event) => onPatch({ reportToFleet: (event.target as HTMLInputElement).checked })}
        label={fleetReportingRequired
          ? 'Fleet reporting required by device policy'
          : 'Report a coarse Wi-Fi Guard alert to Fleet'}
        style={{ marginBottom: 0 }}
      />
      <div style={{ fontSize: 10, color: 'var(--color-text-muted)', lineHeight: 1.4 }}>
        Fleet receives only “rogue access point” severity—not the SSID, BSSID, or baseline stored on this device.
      </div>
    </div>
  );
}

export default function WifiGuardSection({
  embedded = false,
}: {
  expanded?: boolean;
  onExpandedChange?: (next: boolean) => void;
  /** When true, skip the outer SectionCard so the contents can be embedded
   *  inside a sibling card (e.g. the merged Adapters + Wi-Fi Guard card). */
  embedded?: boolean;
} = {}) {
  const requestConfirm = useAppConfirm();
  const { appSettings, patchAppSettings, systemInfo } = useAppState();
  const needsElevation = isPrivilegedWriteBlocked(true, systemInfo?.isAdmin);
  const persisted = appSettings?.ideal?.network?.wifiGuard;
  const configuredEnabled = persisted?.enabled ?? false;
  const learningWindowSecs = persisted?.learningWindowSecs ?? DEFAULT_WIFI_GUARD_LEARNING_WINDOW_SECS;
  const pollIntervalSecs = persisted?.pollIntervalSecs ?? DEFAULT_WIFI_GUARD_POLL_INTERVAL_SECS;
  const alertDebounceSecs = persisted?.alertDebounceSecs ?? DEFAULT_WIFI_GUARD_ALERT_DEBOUNCE_SECS;
  const reportToFleet = persisted?.reportToFleet ?? false;
  const fleetReportingRequired = appSettings?.ideal?.security?.requireAllDeviceAlertsInFleet === true;
  const [status, setStatus] = useState<WifiGuardStatus | null>(null);
  const [recent, setRecent] = useState<WifiGuardHit[]>([]);
  const [known, setKnown] = useState<KnownEntry[]>([]);
  const [toggling, setToggling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showKnown, setShowKnown] = useState(false);
  const [addSsid, setAddSsid] = useState('');
  const [addBssid, setAddBssid] = useState('');
  const [addingSsid, setAddingSsid] = useState(false);
  // Track when we last armed so a near-immediate Refresh doesn't flip running→false
  // before the backend state has propagated (Rust may take ~1-2s to reflect).
  const lastArmRef = useRef(0);

  const patchWifiGuard = useCallback((patch: {
    enabled?: boolean;
    learningWindowSecs?: number;
    pollIntervalSecs?: number;
    alertDebounceSecs?: number;
    learningUntil?: string | null;
    baseline?: WifiGuardBaselineEntry[];
    reportToFleet?: boolean;
  }) => {
    void patchAppSettings({ ideal: { network: { wifiGuard: patch } } }).catch(reportSettingsWriteFailure);
  }, [patchAppSettings]);

  const persistRuntimeBaseline = useCallback(async () => {
    const [baseline, runtime] = await Promise.all([
      invoke<WifiGuardBaselineEntry[]>('get_wifi_guard_baseline'),
      invoke<WifiGuardStatus>('wifi_guard_status'),
    ]);
    patchWifiGuard({ baseline, learningUntil: runtime.learningUntil });
  }, [patchWifiGuard]);

  const refresh = useCallback(async () => {
    try {
      const [s, r, k] = await Promise.all([
        invoke<WifiGuardStatus>('wifi_guard_status'),
        invoke<WifiGuardHit[]>('get_wifi_guard_recent'),
        invoke<KnownEntry[]>('get_wifi_guard_known'),
      ]);
      setStatus((prev) => {
        // Optimistic guard: if we armed within the last 3 s and the backend
        // still says not running, keep the optimistic running=true state so a
        // quick Refresh click doesn't disarm the UI.
        if (!s.running && prev?.running && Date.now() - lastArmRef.current < 3000) {
          return { ...s, running: true };
        }
        return s;
      });
      setRecent([...r].reverse());
      setKnown(k);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
    let unlisten: UnlistenFn | null = null;
    (async () => {
      unlisten = await listen<WifiGuardHit>('wifi-guard-detected', (e) => {
        setRecent((prev) => [e.payload, ...prev].slice(0, 30));
        // Status may change too (learning flag, known count) — refresh.
        void refresh();
      });
    })();
    // Periodically refresh status so the learning-window countdown and
    // current SSID display stay live without a manual poke.
    const interval = setInterval(() => void refresh(), 30_000);
    return () => {
      if (unlisten) unlisten();
      clearInterval(interval);
    };
  }, [refresh]);

  const handleToggle = useCallback(
    async (next: boolean) => {
      if (needsElevation) { setError(MACHINE_SCOPE_ELEVATION_MESSAGE); return; }
      setToggling(true);
      setError(null);
      try {
        if (next) {
          lastArmRef.current = Date.now();
          const s = await invoke<WifiGuardStatus>('start_wifi_guard');
          setStatus(s);
        } else {
          await invoke('stop_wifi_guard');
          await refresh();
        }
        patchWifiGuard({ enabled: next, ...(next ? {} : { learningUntil: null }) });
      } catch (err) {
        const msg = String(err);
        setError(
          msg.includes('PAID:') || msg.toLowerCase().includes('paid')
            ? 'WinCommander Pro required for Rogue AP Guard.'
            : msg,
        );
      } finally {
        setToggling(false);
      }
    },
    [refresh, patchWifiGuard, needsElevation],
  );

  const handleAddSsid = useCallback(async () => {
    if (needsElevation) { setError(MACHINE_SCOPE_ELEVATION_MESSAGE); return; }
    const ssid = addSsid.trim();
    if (!ssid) {
      setError('SSID name is required.');
      return;
    }
    setAddingSsid(true);
    setError(null);
    try {
      const bssid = addBssid.trim() || null;
      const s = await invoke<WifiGuardStatus>('add_wifi_guard_ssid', { ssid, bssid });
      setStatus(s);
      setAddSsid('');
      setAddBssid('');
      await persistRuntimeBaseline();
      await refresh();
    } catch (err) {
      const msg = String(err);
      setError(
        msg.includes('PAID:') || msg.toLowerCase().includes('paid')
          ? 'WinCommander Pro required to manually trust an SSID.'
          : msg,
      );
    } finally {
      setAddingSsid(false);
    }
  }, [addSsid, addBssid, refresh, persistRuntimeBaseline, needsElevation]);

  const handleClearKnown = useCallback(async () => {
    if (needsElevation) { setError(MACHINE_SCOPE_ELEVATION_MESSAGE); return; }
    const accepted = await requestConfirm({
      title: 'Forget learned Wi-Fi identities?',
      description: 'All learned SSID/BSSID associations will be removed. Wi-Fi Guard will re-enter learning mode for 24 hours.',
      confirmLabel: 'Forget associations',
    });
    if (!accepted) return;
    try {
      await invoke('clear_wifi_guard_known');
      await persistRuntimeBaseline();
      await refresh();
    } catch (err) {
      setError(String(err));
    }
  }, [refresh, requestConfirm, persistRuntimeBaseline, needsElevation]);

  const handleClearRecent = useCallback(async () => {
    const accepted = await requestConfirm({
      title: 'Clear recent Wi-Fi Guard fires?',
      description: 'This removes the recent Wi-Fi identity alerts recorded for this app session.',
      confirmLabel: 'Clear alerts',
    });
    if (!accepted) return;
    try {
      await invoke('clear_wifi_guard_recent');
      setRecent([]);
    } catch (err) {
      setError(String(err));
    }
  }, [requestConfirm]);

  const running = !!status?.running;
  const learning = !!status?.learning;

  const statusTag = running ? (
    learning ? (
      <Tag intent="warning" minimal style={{ fontSize: 9 }}>LEARNING</Tag>
    ) : (
      <Tag intent="success" minimal style={{ fontSize: 9 }}>GUARDING</Tag>
    )
  ) : (
    <Tag minimal style={{ fontSize: 9, opacity: 0.6 }}>OFF</Tag>
  );

  const body = (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
        <div style={{ fontSize: 12, color: 'var(--color-text-secondary)', lineHeight: 1.5 }}>
          Detects fake hotspots impersonating your saved Wi-Fi networks.
        </div>

        {/* ── Master toggle ──────────────────────────────────────── */}
        <div style={{ display: 'flex', gap: 12, alignItems: 'center', flexWrap: 'wrap' }}>
          <Switch
            checked={running}
            onChange={(e) => handleToggle((e.target as HTMLInputElement).checked)}
            disabled={toggling || needsElevation}
            label={running ? 'Detector running' : 'Arm detector'}
            large
            style={{ marginBottom: 0 }}
          />
          {statusTag}
          {needsElevation && <span className="text-[10px] text-[var(--warn)]">Requires an administrator</span>}
          {toggling && <Spinner size={14} />}
          <Button icon="refresh" minimal small onClick={refresh}>
            Refresh
          </Button>
        </div>

        {/* ── Current association readout ─────────────────────── */}
        {running && (
          <div
            style={{
              display: 'flex',
              flexDirection: 'column',
              gap: 4,
              padding: 10,
              background: 'var(--color-bg-secondary)',
              border: '1px solid var(--color-border)',
              borderRadius: 4,
            }}
          >
            <div
              style={{
                fontSize: 10,
                fontWeight: 700,
                textTransform: 'uppercase',
                letterSpacing: 0.6,
                color: 'var(--color-text-muted)',
              }}
            >
              Currently associated to
            </div>
            {status?.currentSsid ? (
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                <span style={{ fontWeight: 600, fontSize: 13 }}>{status.currentSsid}</span>
                <code style={{ fontSize: 11, color: 'var(--color-text-muted)' }}>
                  {status.currentBssid ?? '—'}
                </code>
              </div>
            ) : (
              <span style={{ fontSize: 12, color: 'var(--color-text-muted)' }}>
                Not connected to any Wi-Fi network.
              </span>
            )}
          </div>
        )}

        {/* ── Learning indicator ─────────────────────────────── */}
        {running && learning && (
          <IntelNotice intent="warning" icon="learning" title="Learning window active">
            The detector is observing your Wi-Fi associations and
            won&apos;t fire yet. After {formatDuration(learningWindowSecs)} every SSID it has seen
            becomes a guarded baseline.
          </IntelNotice>
        )}

        <WifiGuardPolicyControls
          enabled={configuredEnabled}
          learningWindowSecs={learningWindowSecs}
          pollIntervalSecs={pollIntervalSecs}
          alertDebounceSecs={alertDebounceSecs}
          reportToFleet={reportToFleet}
          fleetReportingRequired={fleetReportingRequired}
          onPatch={patchWifiGuard}
        />

        {error && (
          <IntelNotice intent="danger" icon="error" title="Detector error">
            {error}
          </IntelNotice>
        )}

        {/* ── Manual SSID add ──────────────────────────────── */}
        <div
          style={{
            display: 'flex',
            gap: 8,
            alignItems: 'center',
            padding: 8,
            background: 'var(--color-bg-secondary)',
            border: '1px solid var(--color-border)',
            borderRadius: 4,
            flexWrap: 'wrap',
          }}
        >
          <InputGroup
            placeholder="SSID name"
            value={addSsid}
            onChange={(e) => setAddSsid((e.target as HTMLInputElement).value)}
            small
            style={{ flex: '1 1 120px', minWidth: 0 }}
          />
          <InputGroup
            placeholder="BSSID (optional)"
            value={addBssid}
            onChange={(e) => setAddBssid((e.target as HTMLInputElement).value)}
            small
            style={{ flex: '1 1 120px', minWidth: 0 }}
          />
          <Button
            icon="add"
            intent="primary"
            onClick={handleAddSsid}
            loading={addingSsid}
            disabled={!addSsid.trim() || addingSsid || needsElevation}
            small
          >
            Trust SSID
          </Button>
        </div>

        {/* ── Known SSIDs (collapsible) ───────────────────────── */}
        <div>
          <div
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center',
              marginBottom: 6,
            }}
          >
            <span
              style={{
                fontSize: 10,
                fontWeight: 700,
                textTransform: 'uppercase',
                letterSpacing: 0.6,
                color: 'var(--color-text-muted)',
              }}
            >
              Known SSIDs ({known.length})
            </span>
            <div style={{ display: 'flex', gap: 4 }}>
              <Button
                icon={showKnown ? 'chevron-up' : 'chevron-down'}
                minimal
                small
                onClick={() => setShowKnown((p) => !p)}
                disabled={known.length === 0}
              >
                {showKnown ? 'Hide' : 'Show'}
              </Button>
              {known.length > 0 && (
                <Button icon="reset" minimal small onClick={handleClearKnown} disabled={needsElevation}>
                  Reset
                </Button>
              )}
            </div>
          </div>
          {showKnown && known.length > 0 && (
            <div
              style={{
                display: 'flex',
                flexDirection: 'column',
                gap: 4,
                maxHeight: 180,
                overflowY: 'auto',
              }}
            >
              {known.map(([ssid, bssids]) => (
                <div
                  key={ssid}
                  style={{
                    padding: '6px 10px',
                    background: 'var(--color-bg-secondary)',
                    border: '1px solid var(--color-border)',
                    borderRadius: 4,
                    fontSize: 11,
                  }}
                >
                  <div style={{ fontWeight: 600, marginBottom: 2 }}>{ssid}</div>
                  <div
                    style={{
                      display: 'flex',
                      flexWrap: 'wrap',
                      gap: 4,
                    }}
                  >
                    {bssids.map((b) => (
                      <Tag key={b} minimal style={{ fontSize: 10, fontFamily: 'var(--font-mono)' }}>
                        {b}
                      </Tag>
                    ))}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* ── Recent fires ────────────────────────────────────── */}
        <div>
          <div
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center',
              marginBottom: 6,
            }}
          >
            <span
              style={{
                fontSize: 10,
                fontWeight: 700,
                textTransform: 'uppercase',
                letterSpacing: 0.6,
                color: 'var(--color-text-muted)',
              }}
            >
              Recent fires ({recent.length})
            </span>
            {recent.length > 0 && (
              <Button icon="trash" minimal small onClick={handleClearRecent}>
                Clear
              </Button>
            )}
          </div>
          {recent.length === 0 ? (
            <div
              style={{
                fontSize: 11,
                color: 'var(--color-text-muted)',
                fontStyle: 'italic',
                padding: '8px 0',
              }}
            >
              {running
                ? learning
                  ? 'Detector is still learning — fires start after the 24 h window.'
                  : 'No rogue AP alerts recorded. The detector is silent until something looks wrong.'
                : 'Arm the detector to start watching.'}
            </div>
          ) : (
            <div
              style={{
                display: 'flex',
                flexDirection: 'column',
                gap: 4,
                maxHeight: 280,
                overflowY: 'auto',
              }}
            >
              {recent.map((h, i) => {
                const r = reasonLabel(h.reason);
                return (
                  <div
                    key={`${h.detectedAt}-${i}`}
                    style={{
                      padding: '6px 10px',
                      background: 'var(--color-bg-secondary)',
                      border: '1px solid color-mix(in srgb, var(--color-danger) 25%, transparent)',
                      borderLeft: '3px solid var(--color-danger)',
                      borderRadius: 4,
                      fontSize: 11,
                    }}
                  >
                    <div
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        gap: 8,
                        flexWrap: 'wrap',
                      }}
                    >
                      <Tag intent={r.intent} minimal style={{ fontSize: 9, fontWeight: 700 }}>
                        {r.label}
                      </Tag>
                      <strong>{h.ssid}</strong>
                      <span style={{ flex: 1 }} />
                      <span
                        style={{
                          fontFamily: 'var(--font-mono)',
                          fontSize: 10,
                          color: 'var(--color-text-muted)',
                        }}
                      >
                        {formatTime(h.detectedAt)}
                      </span>
                    </div>
                    <div
                      style={{
                        marginTop: 3,
                        fontSize: 10,
                        color: 'var(--color-text-muted)',
                        fontFamily: 'var(--font-mono)',
                      }}
                    >
                      {h.bssid} · {h.auth || 'auth?'} · {h.signal || '—'}
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>
  );

  if (embedded) {
    return (
      <div className="wifi-guard-embedded" style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
        <header className="merged-segment__header wifi-guard-embedded__header">
          <Icon icon="cell-tower" size={11} />
          <span className="merged-segment__title">Wi-Fi Guard</span>
          <Switch
            checked={running}
            onChange={(e) => handleToggle((e.target as HTMLInputElement).checked)}
            disabled={toggling || needsElevation}
            label={running ? 'On' : 'Off'}
            style={{ marginBottom: 0 }}
          />
          {toggling && <Spinner size={12} />}
        </header>

        {/* Current association readout — only when armed */}
        {running && (
          <div style={{
            fontSize: 11,
            fontFamily: 'var(--font-mono)',
            color: 'var(--color-text-secondary)',
            padding: '5px 8px',
            background: 'var(--color-bg-secondary)',
            border: '1px solid var(--color-border)',
            borderRadius: 3,
          }}>
            <span style={{ color: 'var(--color-text-muted)', marginRight: 4 }}>connected:</span>
            {status?.currentSsid
              ? <>{status.currentSsid}{status.currentBssid && <span style={{ color: 'var(--color-text-muted)', marginLeft: 6 }}>{status.currentBssid}</span>}</>
              : <span style={{ color: 'var(--color-text-muted)' }}>not connected</span>
            }
          </div>
        )}

        <div className="wifi-guard-manual-row">
          <InputGroup
            placeholder="SSID"
            value={addSsid}
            onChange={(e) => setAddSsid((e.target as HTMLInputElement).value)}
            small
          />
          <InputGroup
            placeholder="BSSID optional"
            value={addBssid}
            onChange={(e) => setAddBssid((e.target as HTMLInputElement).value)}
            small
          />
          <Button
            icon="add"
            intent="primary"
            onClick={handleAddSsid}
            loading={addingSsid}
            disabled={!addSsid.trim() || addingSsid || needsElevation}
            small
          >
            Trust
          </Button>
        </div>

        {/* Learning indicator */}
        {running && learning && (
          <IntelNotice intent="warning" icon="time" title="Learning mode">
            Passively learning BSSID associations. Full detection activates after {formatDuration(learningWindowSecs)} of
            trusted-network observations.
          </IntelNotice>
        )}

        <WifiGuardPolicyControls
          enabled={configuredEnabled}
          learningWindowSecs={learningWindowSecs}
          pollIntervalSecs={pollIntervalSecs}
          alertDebounceSecs={alertDebounceSecs}
          reportToFleet={reportToFleet}
          fleetReportingRequired={fleetReportingRequired}
          onPatch={patchWifiGuard}
        />

        {/* Recent fires — compact, last 2 */}
        {recent.length > 0 && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
            <span style={{ fontSize: 10, fontWeight: 700, textTransform: 'uppercase', letterSpacing: 0.6, color: 'var(--color-text-muted)' }}>
              Recent alerts
            </span>
            {recent.slice(0, 2).map((h, i) => (
              <div key={i} style={{ fontSize: 10, fontFamily: 'var(--font-mono)', color: 'var(--color-danger)', padding: '3px 6px', background: 'color-mix(in srgb, var(--color-danger) 10%, transparent)', borderRadius: 3, border: '1px solid color-mix(in srgb, var(--color-danger) 30%, transparent)' }}>
                {h.ssid} · {h.bssid}
              </div>
            ))}
          </div>
        )}

        {error && (
          <IntelNotice intent="danger" icon="error" title="Error">
            {error}
          </IntelNotice>
        )}
      </div>
    );
  }

  return (
    <SectionCard
      title="Wi-Fi Guard"
      icon="cell-tower"
    >
      {body}
    </SectionCard>
  );
}
