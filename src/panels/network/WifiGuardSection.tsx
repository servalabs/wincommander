import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Button, InputGroup, Switch, Tag } from '@/components/ui/bp';
import SectionCard from '../../components/shared/SectionCard';
import { useAppConfirm } from '../../components/shared/AppConfirmDialog';
import { useAppState } from '../../context/AppContext';
import { isPrivilegedWriteBlocked, MACHINE_SCOPE_ELEVATION_MESSAGE } from '../../lib/machineScopeElevation';
import { isFleetReportControlLocked } from '../../lib/fleetReportingPolicyLock';
import { applyWifiApproval, canTrustAssociation, isWifiGuardRecoveryBlocked, setWifiGuardRecoveryBlocked, wifiBaselineIdentity, wifiReasonLabel, wifiSecurityLabel, wifiStatusLabel, withWifiGuardOperation, type WifiGuardAssociation, type WifiGuardHit, type WifiGuardStatus } from '../../lib/wifiGuard';
import type { AppSettings, WifiGuardBaselineEntry } from '../../types/settings';

export default function WifiGuardSection({ embedded = false }: { expanded?: boolean; onExpandedChange?: (next: boolean) => void; embedded?: boolean } = {}) {
  const { appSettings, patchAppSettings, refreshSettings, systemInfo } = useAppState();
  const requestConfirm = useAppConfirm();
  const settings = appSettings?.ideal?.network?.wifiGuard;
  const settingsRef = useRef(settings); settingsRef.current = settings;
  const needsElevation = isPrivilegedWriteBlocked(true, systemInfo?.isAdmin);
  const enabled = settings?.enabled ?? false;
  const baseline = settings?.baseline ?? [];
  const required = appSettings?.ideal?.security?.requireAllDeviceAlertsInFleet === true;
  const reportingLocked = isFleetReportControlLocked({ lockedPaths: appSettings?.policy?.lockedPaths, reportPath: 'network.wifiGuard.reportToFleet', requireAllDeviceAlertsInFleet: required });
  const [status, setStatus] = useState<WifiGuardStatus | null>(null);
  const [recent, setRecent] = useState<WifiGuardHit[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [readError, setReadError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [showKnown, setShowKnown] = useState(false);
  const refreshPending = useRef(false);
  const mounted = useRef(true);

  const refresh = useCallback(async () => {
    if (refreshPending.current) return;
    refreshPending.current = true;
    try {
      const [stateResult, recentResult] = await Promise.allSettled([
        invoke<WifiGuardStatus>('wifi_guard_status'), invoke<WifiGuardHit[]>('get_wifi_guard_recent'),
      ]);
      if (!mounted.current) return;
      setReadError(null);
      if (stateResult.status === 'fulfilled') setStatus(stateResult.value);
      else { setStatus(null); setReadError(`Wi-Fi observation unavailable: ${String(stateResult.reason)}`); }
      if (recentResult.status === 'fulfilled') setRecent([...recentResult.value].reverse());
      else setReadError(`Recent Wi-Fi alerts could not be read: ${String(recentResult.reason)}`);
    } finally { refreshPending.current = false; }
  }, []);

  useEffect(() => {
    mounted.current = true;
    let disposed = false;
    const subscription = listen<WifiGuardHit>('wifi-guard-detected', () => { if (!disposed) void refresh(); });
    void refresh();
    const timer = window.setInterval(() => void refresh(), 10_000);
    return () => { disposed = true; mounted.current = false; window.clearInterval(timer); void subscription.then((unlisten) => unlisten()).catch(() => undefined); };
  }, [refresh]);

  const mutate = async (operation: () => Promise<void>) => {
    if (needsElevation) { setError(MACHINE_SCOPE_ELEVATION_MESSAGE); return; }
    setBusy(true); setError(null);
    try { await withWifiGuardOperation(operation); await refresh(); }
    catch (failure) { setError(String(failure)); }
    finally { setBusy(false); }
  };

  const policy = (saved = settingsRef.current) => ({
    learningWindowSecs: saved?.learningWindowSecs ?? 86400,
    learningUntil: null,
    pollIntervalSecs: saved?.pollIntervalSecs ?? 10,
    alertDebounceSecs: saved?.alertDebounceSecs ?? 300,
    baseline: saved?.baseline ?? [],
  });

  const recoverSavedTrust = async () => {
    const authoritative = await invoke<AppSettings>('get_settings');
    const saved = authoritative.ideal?.network?.wifiGuard;
    await invoke('configure_wifi_guard', { config: policy(saved ?? {}) });
    if (!saved?.enabled) await invoke('stop_wifi_guard');
    await refreshSettings();
    setWifiGuardRecoveryBlocked(false);
    return saved;
  };

  const approve = async (row: WifiGuardAssociation) => {
    const accepted = await requestConfirm({
      title: 'Trust this Wi-Fi access point?',
      description: `Approve network “${row.ssid}” and access point ${row.bssid} on ${row.name}, using ${wifiSecurityLabel(row)}. ${row.trust === 'authDowngrade' ? 'This is weaker than the previous approval. ' : ''}Confirm this is your router or a trusted access point. A matching name alone does not prove ownership.`,
      confirmLabel: 'Trust and enable guard',
    });
    if (!accepted) return;
    await mutate(async () => {
      const before = policy();
      await invoke('configure_wifi_guard', { config: before });
      let intendedBaseline: WifiGuardBaselineEntry[] | null = null;
      await applyWifiApproval({
        trust: () => invoke('trust_wifi_guard_current', { interfaceId: row.interfaceId, expectedSsidHex: row.ssidHex, expectedBssid: row.bssid, expectedAuthAlgorithm: row.authAlgorithm, expectedCipherAlgorithm: row.cipherAlgorithm }),
        persist: async () => {
          const approved = await invoke<WifiGuardBaselineEntry[]>('get_wifi_guard_baseline');
          intendedBaseline = approved;
          await patchAppSettings({ ideal: { network: { wifiGuard: { enabled: true, baseline: approved, learningUntil: null } } } });
        },
        restore: async () => {
          const saved = await recoverSavedTrust();
          return intendedBaseline && saved?.enabled && wifiBaselineIdentity(saved.baseline ?? []) === wifiBaselineIdentity(intendedBaseline) ? 'saved' : 'restored';
        },
        stop: () => invoke('stop_wifi_guard'),
      });
    });
  };

  const toggle = (next: boolean) => mutate(async () => {
    await patchAppSettings({ ideal: { network: { wifiGuard: { enabled: next } } } });
    if (!next) await invoke('stop_wifi_guard');
  });

  const clearKnown = async () => {
    if (!await requestConfirm({ title: 'Forget Wi-Fi approvals?', description: 'Saved networks remain connected, but every access point will need approval again. This does not change Windows Wi-Fi profiles.', confirmLabel: 'Forget approvals' })) return;
    await mutate(async () => {
      await patchAppSettings({ ideal: { network: { wifiGuard: { baseline: [], learningUntil: null } } } });
      await invoke('clear_wifi_guard_known');
    });
  };

  const label = readError && !status ? 'Wi-Fi observation unavailable' : wifiStatusLabel(status);
  const guarding = label === 'Guarding approved Wi-Fi';
  const body = <div className={embedded ? 'wifi-guard-embedded' : undefined} style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
    <div style={{ display: 'flex', gap: 10, alignItems: 'center', flexWrap: 'wrap' }}>
      {embedded && <strong>Wi-Fi Guard</strong>}
      <Switch checked={enabled} disabled={busy || needsElevation} onChange={(event) => void toggle((event.target as HTMLInputElement).checked)} label="Enable Wi-Fi Guard" style={{ marginBottom: 0 }} />
      <Tag intent={guarding ? 'success' : 'warning'}>{label}</Tag>
      <Button minimal small icon="refresh" onClick={() => void refresh()}>Refresh</Button>
    </div>
    <p style={{ margin: 0, fontSize: 12 }}>Checks the Wi-Fi connection while WinCommander is running. SSID is the network name; BSSID identifies the access point. Alerts flag a change to review and do not prove that a hotspot is malicious.</p>
    {enabled && !status?.running && <p role="status" style={{ margin: 0 }}>Guard requested; waiting for its observer to start.</p>}
    {!enabled && status?.running && <p role="alert" style={{ margin: 0 }}>Stop requested; Windows has not yet confirmed that the observer stopped. Automatic retry is active.</p>}
    {status?.health === 'permissionDenied' && <p role="status" style={{ margin: 0 }}>Windows denied access to Wi-Fi details. Check Settings → Privacy &amp; security → Location and your administrator’s policy, then Refresh. The app does not change those permissions.</p>}
    {needsElevation && <p role="status">{MACHINE_SCOPE_ELEVATION_MESSAGE}</p>}
    {error && <p role="alert" style={{ margin: 0, color: 'var(--color-danger)' }}>{error}</p>}
    {readError && <p role="alert" style={{ margin: 0, color: 'var(--color-danger)' }}>{readError}</p>}
    {isWifiGuardRecoveryBlocked() && <div role="alert">Saved Wi-Fi trust could not be confirmed. Automatic re-arming is paused. <Button small disabled={busy || needsElevation} onClick={() => void mutate(async () => { await recoverSavedTrust(); })}>Retry saved trust recovery</Button></div>}

    {(status?.interfaces ?? []).map((row) => <div key={row.interfaceId} style={{ padding: 10, border: '1px solid var(--color-border)', borderRadius: 4 }}>
      <strong>{row.name}</strong>
      {row.state === 'connected' ? <>
        <div style={{ whiteSpace: 'pre-wrap' }}>Network name (SSID): {row.ssid}</div>
        <div>Access point (BSSID): <code>{row.bssid}</code> · Signal {row.signal}%</div>
        <div>Security: {wifiSecurityLabel(row)}</div>
        <div>{row.trust === 'approved' ? 'Approved access point' : wifiReasonLabel(row.trust)}</div>
        {row.trust !== 'approved' && <Button small intent="primary" disabled={busy || needsElevation || !canTrustAssociation(row)} onClick={() => void approve(row)}>Trust current access point and enable</Button>}
      </> : <div>{row.state === 'disconnected' ? 'Disconnected' : row.state === 'reconnecting' ? 'Reconnecting' : 'Wi-Fi information unavailable'}</div>}
    </div>)}
    {status?.observedAt && <small>Last checked: {new Date(status.observedAt).toLocaleString()}. Turning this PC’s MAC randomization on does not change the router’s BSSID.</small>}

    <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
      <label>Check every (seconds)<InputGroup type="number" min={5} max={300} value={String(settings?.pollIntervalSecs ?? 10)} disabled={busy || needsElevation} onChange={(event) => {
        const value = Number(event.target.value);
        if (Number.isInteger(value) && value >= 5 && value <= 300) void mutate(async () => { await patchAppSettings({ ideal: { network: { wifiGuard: { pollIntervalSecs: value } } } }); });
      }} /></label>
      <label>Repeat alert (seconds)<InputGroup type="number" min={30} max={3600} value={String(settings?.alertDebounceSecs ?? 300)} disabled={busy || needsElevation} onChange={(event) => {
        const value = Number(event.target.value);
        if (Number.isInteger(value) && value >= 30 && value <= 3600) void mutate(async () => { await patchAppSettings({ ideal: { network: { wifiGuard: { alertDebounceSecs: value } } } }); });
      }} /></label>
    </div>
    <Switch checked={required || settings?.reportToFleet === true} disabled={reportingLocked || busy || needsElevation} label={reportingLocked ? 'Fleet reporting required by device policy' : 'Report coarse Wi-Fi alerts to Fleet'} onChange={(event) => {
      const reportToFleet = (event.target as HTMLInputElement).checked;
      void mutate(async () => { await patchAppSettings({ ideal: { network: { wifiGuard: { reportToFleet } } } }); });
    }} />
    <small>Network names and access-point identities stay in local settings; Fleet receives only an alert category. Popup delivery follows your notification settings.</small>

    <div>
      <Button minimal small onClick={() => setShowKnown(!showKnown)}>{showKnown ? 'Hide' : 'Show'} saved Wi-Fi identities ({baseline.length})</Button>
      {baseline.length > 0 && <Button minimal small intent="danger" disabled={busy || needsElevation} onClick={() => void clearKnown()}>Forget approvals</Button>}
      {showKnown && baseline.map((entry, index) => <div key={`${entry.ssidHex ?? entry.ssid}-${index}`} style={{ padding: 8, borderBottom: '1px solid var(--color-border)' }}>
        <strong style={{ whiteSpace: 'pre-wrap' }}>{entry.ssid}</strong> · {entry.provenance === 'approved' ? 'Approved' : 'Learned previously — needs review'}
        <div>{entry.bssids.join(', ') || 'No access point recorded'}</div>
      </div>)}
      {showKnown && <small>Mesh routers may have several access points. Approve each one you recognize; new ones are never silently trusted.</small>}
    </div>

    <div><strong>Recent Wi-Fi alerts ({recent.length})</strong>
      {recent.length > 0 && <Button minimal small disabled={busy} onClick={() => void mutate(async () => { await invoke('clear_wifi_guard_recent'); setRecent([]); })}>Clear alerts</Button>}
      {recent.length === 0 ? <p>No alerts recorded in this observer session. This is not proof of a trusted connection.</p> : <div style={{ maxHeight: 240, overflowY: 'auto' }}>{recent.map((hit, index) => <div key={hit.id || `${hit.detectedAt}-${index}`} style={{ padding: 8, borderBottom: '1px solid var(--color-border)' }}>
        <strong>{wifiReasonLabel(hit.reason)}</strong> · {new Date(hit.detectedAt).toLocaleString()}
        <div>{hit.ssid} · <code>{hit.bssid}</code></div>
      </div>)}</div>}
      <small>Up to 100 alerts are retained for this app-running observer session.</small>
    </div>
  </div>;
  return embedded ? body : <SectionCard title="Wi-Fi Guard" icon="cell-tower">{body}</SectionCard>;
}
