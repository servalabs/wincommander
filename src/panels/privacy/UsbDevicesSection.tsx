import { useCallback, useEffect, useId, useMemo, useState } from 'react';
import type { ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Button, Spinner, Switch, Tag } from '@/components/ui/bp';
import type { Intent } from '@/components/ui/bp';
import SectionCard from '../../components/shared/SectionCard';
import PrivacyEventTable from './PrivacyEventTable';
import TierGate from '../../components/shared/TierGate';
import useEntitlements from '../../hooks/useEntitlements';
import { useAppConfirm } from '../../components/shared/AppConfirmDialog';
import { useAppState } from '../../context/AppContext';
import { reportSettingsWriteFailure } from '../../lib/settingsWriteRecovery';
import { formatTrustScore, trustScoreTone } from '../../lib/usbTrust';
import { showError, showSuccess } from '../../utils/toast';
import { useUsbHidApproval } from '../../context/UsbHidApprovalContext';
import { DEFAULT_USB_HID_APPROVAL_TTL_SECS } from '../../lib/usbHidApproval';
import UsbHidApprovalGateSettings from './UsbHidApprovalGateSettings';
import { newDiagnosticOperationId, recordDiagnostic } from '../../lib/diagnostics';

interface UsbMonitorFailure {
  code: string;
  message: string;
  recoveryAction: string;
}

interface UsbMonitorStatus {
  running: boolean;
  notify: boolean;
  connected?: number;
  monitorStartedAt?: number | null;
  lastPollAt?: number | null;
  lastError?: UsbMonitorFailure | null;
  historyCoverage?: 'monitorOnly' | string;
  windowsHistoryAvailable?: boolean;
}

interface UsbDeviceIdentity {
  key: string;
  vid: string;
  pid: string;
  friendlyName: string;
  isHid: boolean;
  isMassStorage: boolean;
  instanceId?: string;
}

interface UsbDeviceRecord {
  identity: UsbDeviceIdentity;
  firstSeen?: number;
  lastSeen: number;
  totalPluggedSecs: number;
  sessionCount: number;
}

interface UsbSessionRow {
  deviceKey: string;
  attachedAt: number;
  detachedAt: number | null;
  durationSecs?: number | null;
  volumeLetter?: string | null;
  attachedAtEstimated?: boolean;
  endedUnobservedAt?: number | null;
}

interface UsbTimeline {
  records: Record<string, UsbDeviceRecord>;
  sessions: UsbSessionRow[];
  currentKeys?: string[];
  monitorStartedAt?: number | null;
  lastPollAt?: number | null;
  historyCoverage?: 'monitorOnly' | string;
  windowsHistoryAvailable?: boolean;
}

type DeviceCategory = 'Keyboard / HID' | 'Storage' | 'USB device';

interface UsbTimelineEntry {
  key: string;
  instanceId: string;
  friendlyName: string;
  category: DeviceCategory;
  lastSeen: number;
  totalPluggedSecs: number;
  sessionCount: number;
  attached: boolean;
  driveLetter: string | null;
  openSinceEpoch: number | null;
}

type TimelineState = 'Connected now' | 'Attached' | 'Detached' | 'Present when armed' | 'State unknown';
type TimelineSource = 'Current monitor run' | 'Persisted monitor record';

interface TimelineEvent {
  id: string;
  deviceKey: string;
  name: string;
  category: DeviceCategory;
  state: TimelineState;
  at: number;
  durationSecs: number | null;
  source: TimelineSource;
}

interface UsbTrustScore {
  deviceKey: string;
  score: number;
  signals: {
    serialStable: boolean;
    isHid: boolean;
    isMassStorage: boolean;
    knownVendor: boolean;
    hidAlerts: number;
    quarantineActions: number;
    transferBytes: number;
  };
}

interface UsbTransferStat {
  deviceKey: string;
  friendlyName: string;
  readBytes: number;
  writeBytes: number;
  lastSampleEpoch: number;
}

interface UsbVolume {
  driveLetter: string;
  label: string;
  model: string;
  serial: string;
}

interface HidInjectionAlert {
  deviceKey: string;
  friendlyName: string;
  detectedAt: string;
  gapsSampled: number;
  medianGapMs: number;
  recentHidDevice: string | null;
  redFlag: 'hidOnly' | 'composite' | 'unknown';
  severity: 'danger' | 'warning';
}

type AutoSandboxMode = 'off' | 'observe' | 'enforce';

interface AutoSandboxStatus {
  running: boolean;
  mode: AutoSandboxMode;
  recentCount: number;
  allowKeys: string[];
  allowVids: string[];
  actOnHid: boolean;
}

interface AutoActionRecord {
  time: string;
  deviceKey: string;
  friendlyName: string;
  action: 'ignore' | 'alert' | 'quarantine';
  enforced: boolean;
  detail: string;
}

function InfoButton({ label, children }: { label: string; children: ReactNode }) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const [pinned, setPinned] = useState(false);

  const close = useCallback(() => {
    setOpen(false);
    setPinned(false);
  }, []);

  return (
    <span className="relative inline-flex items-center">
      <button
        type="button"
        className="inline-flex h-5 w-5 items-center justify-center rounded-full border border-white/15 text-[11px] opacity-70 hover:opacity-100 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2"
        aria-label={label}
        aria-expanded={open}
        aria-describedby={open ? id : undefined}
        onMouseEnter={() => setOpen(true)}
        onMouseLeave={() => { if (!pinned) setOpen(false); }}
        onFocus={() => setOpen(true)}
        onBlur={() => { if (!pinned) setOpen(false); }}
        onClick={() => {
          const next = !open;
          setPinned(next);
          setOpen(next);
        }}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.preventDefault();
            close();
            event.currentTarget.blur();
          }
        }}
      >
        i
      </button>
      {open && (
        <span
          id={id}
          role="tooltip"
          className="absolute left-6 top-0 z-30 w-72 rounded-md border border-white/15 bg-[var(--surface)] p-2 text-xs leading-relaxed shadow-xl"
        >
          {children}
        </span>
      )}
    </span>
  );
}

function formatBytes(value: number): string {
  if (value < 1024) return `${value} B`;
  if (value < 1024 ** 2) return `${(value / 1024).toFixed(1)} KB`;
  if (value < 1024 ** 3) return `${(value / 1024 ** 2).toFixed(1)} MB`;
  return `${(value / 1024 ** 3).toFixed(2)} GB`;
}

function formatDuration(secs: number | null): string {
  if (secs == null) return 'Unavailable';
  if (secs < 60) return `${Math.max(0, secs)}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ${secs % 60}s`;
  return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
}

function safeDate(epochSeconds: number): string {
  if (!Number.isFinite(epochSeconds) || epochSeconds <= 0) return 'Unknown time';
  return new Date(epochSeconds * 1000).toLocaleString();
}

function safeFriendlyName(identity: UsbDeviceIdentity): string {
  const supplied = identity.friendlyName?.trim();
  if (supplied) return supplied;
  if (identity.isMassStorage) return 'USB storage device';
  if (identity.isHid) return 'USB input device';
  return 'USB device';
}

function categoryFor(identity: UsbDeviceIdentity): DeviceCategory {
  if (identity.isMassStorage) return 'Storage';
  if (identity.isHid) return 'Keyboard / HID';
  return 'USB device';
}

function isInternalUsbPlumbing(identity: UsbDeviceIdentity): boolean {
  if (identity.isMassStorage || identity.isHid) return false;
  const label = `${identity.friendlyName ?? ''} ${identity.instanceId ?? ''}`.toLowerCase();
  return /root hub|host controller|generic usb hub|usb composite device|usb hub|xhci|ehci|ohci|uhci/.test(label);
}

function humanizeUsbError(error: unknown): string {
  const value = String(error).replace(/^Error:\s*/i, '');
  if (value.includes('PRO_NOT_INSTALLED')) {
    return 'This action needs WinCommander Pro installed. Open Settings → Pro to install it.';
  }
  if (/access denied|permission/i.test(value)) {
    return 'Windows denied USB device access. Run WinCommander with the required administrator permissions, then retry.';
  }
  if (/timed out/i.test(value)) {
    return 'The Windows USB device query timed out. Refresh once; if it repeats, restart WinCommander and check the Plug and Play service.';
  }
  if (/usb timeline|machine-state|programdata/i.test(value)) {
    return 'Stored USB monitor data could not be read or saved. Check ProgramData permissions and free disk space, then refresh.';
  }
  return value;
}

function volumeForEntry(entry: UsbTimelineEntry, volumes: UsbVolume[]): UsbVolume | undefined {
  if (entry.category !== 'Storage' || volumes.length === 0) return undefined;
  if (entry.driveLetter) {
    const direct = volumes.find((volume) => volume.driveLetter === entry.driveLetter);
    if (direct) return direct;
  }
  if (volumes.length === 1) return volumes[0];
  return undefined;
}

function displayNameForEntry(entry: UsbTimelineEntry, volume: UsbVolume | undefined): string {
  if (!volume) return entry.friendlyName;
  return `${volume.label || volume.model || 'USB Drive'} (${volume.driveLetter})`;
}

function stateIntent(state: TimelineState): Intent | undefined {
  if (state === 'Connected now') return 'success';
  if (state === 'Detached') return undefined;
  if (state === 'State unknown') return 'warning';
  return 'primary';
}

function isToday(value: string): boolean {
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) return false;
  const now = new Date();
  return parsed.getFullYear() === now.getFullYear()
    && parsed.getMonth() === now.getMonth()
    && parsed.getDate() === now.getDate();
}

export default function UsbDevicesSection() {
  const { hasPaid, isLoading: entitlementLoading } = useEntitlements();
  const advancedAvailable = hasPaid && !entitlementLoading;
  const { appSettings, patchAppSettings } = useAppState();
  const requestConfirm = useAppConfirm();
  const { status: hidApprovalStatus, start: startHidApprovalGate, stop: stopHidApprovalGate } = useUsbHidApproval();

  const [status, setStatus] = useState<UsbMonitorStatus>({ running: false, notify: true });
  const [entries, setEntries] = useState<UsbTimelineEntry[]>([]);
  const [sessions, setSessions] = useState<UsbSessionRow[]>([]);
  const [currentKeys, setCurrentKeys] = useState<Set<string>>(new Set());
  const [monitorStartedAt, setMonitorStartedAt] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [volumes, setVolumes] = useState<UsbVolume[]>([]);
  const [trustScores, setTrustScores] = useState<Record<string, UsbTrustScore>>({});
  const [blockedKeys, setBlockedKeys] = useState<Set<string>>(new Set());
  const [proInstalled, setProInstalled] = useState(true);
  const [nowSec, setNowSec] = useState(() => Math.floor(Date.now() / 1000));

  const [metering, setMetering] = useState(false);
  const [stats, setStats] = useState<UsbTransferStat[]>([]);
  const [hidGuardRunning, setHidGuardRunning] = useState(false);
  const [hidSensitivity, setHidSensitivity] = useState<'lenient' | 'balanced' | 'strict'>('balanced');
  const [hidThresholds, setHidThresholds] = useState({ humanFloorMs: 30, minBurstKeys: 12 });
  const [hidAlerts, setHidAlerts] = useState<HidInjectionAlert[]>([]);
  const [autoSandboxRunning, setAutoSandboxRunning] = useState(false);
  const [autoSandboxMode, setAutoSandboxMode] = useState<AutoSandboxMode>('observe');
  const [autoSandboxConfig, setAutoSandboxConfig] = useState({ allowKeys: [] as string[], allowVids: [] as string[], actOnHid: false });
  const [autoActions, setAutoActions] = useState<AutoActionRecord[]>([]);
  const [autoSandboxBusy, setAutoSandboxBusy] = useState(false);
  const [hidApprovalBusy, setHidApprovalBusy] = useState(false);

  const hidApprovalGateEnabled = appSettings?.ideal?.privacy?.usbSecurity?.hidApprovalGateEnabled === true;
  const hidApprovalTtlSecs = appSettings?.ideal?.privacy?.usbSecurity?.hidApprovalTtlSecs
    ?? DEFAULT_USB_HID_APPROVAL_TTL_SECS;

  const recordUsbFailure = useCallback((action: string, errorCode: string, attempted = true) => {
    recordDiagnostic({
      operationId: newDiagnosticOperationId('usb'), feature: 'usb', action, stage: attempted ? 'windows_apply' : 'validation',
      lifecycle: attempted ? 'applied' : 'acknowledged', outcome: 'failed', errorCode, severity: 'warn', retryability: 'manual',
      suggestedNextAction: 'retry', privacyClass: 'local_sensitive',
    });
  }, []);

  const refreshVolumes = useCallback(async () => {
    try {
      const result = await invoke<UsbVolume[]>('get_usb_storage_volumes');
      setVolumes(Array.isArray(result) ? result : []);
    } catch {
      setVolumes([]);
    }
  }, []);

  const refreshAdvanced = useCallback(async (visibleEntries: UsbTimelineEntry[]) => {
    if (!advancedAvailable) {
      setTrustScores({});
      setMetering(false);
      setStats([]);
      setHidGuardRunning(false);
      setHidAlerts([]);
      setAutoSandboxRunning(false);
      setAutoActions([]);
      return;
    }

    const [meterStatus, transferStats, hidStatus, alerts, sandboxStatus, sandboxRecent] = await Promise.allSettled([
      invoke<boolean>('usb_metering_status'),
      invoke<UsbTransferStat[]>('get_usb_transfer_stats'),
      invoke<{ running: boolean; sensitivity?: 'lenient' | 'balanced' | 'strict'; humanFloorMs?: number; minBurstKeys?: number }>('usb_hid_guard_status'),
      invoke<HidInjectionAlert[]>('get_usb_hid_alerts'),
      invoke<AutoSandboxStatus>('usb_autosandbox_status'),
      invoke<AutoActionRecord[]>('get_usb_autosandbox_recent'),
    ]);

    if (meterStatus.status === 'fulfilled') setMetering(!!meterStatus.value);
    if (transferStats.status === 'fulfilled') setStats(Array.isArray(transferStats.value) ? transferStats.value : []);
    if (hidStatus.status === 'fulfilled') {
      setHidGuardRunning(!!hidStatus.value?.running);
      setHidSensitivity(hidStatus.value?.sensitivity ?? 'balanced');
      setHidThresholds({
        humanFloorMs: hidStatus.value?.humanFloorMs ?? 30,
        minBurstKeys: hidStatus.value?.minBurstKeys ?? 12,
      });
    }
    if (alerts.status === 'fulfilled') setHidAlerts(Array.isArray(alerts.value) ? alerts.value : []);
    if (sandboxStatus.status === 'fulfilled') {
      setAutoSandboxRunning(!!sandboxStatus.value?.running);
      setAutoSandboxMode(sandboxStatus.value?.mode ?? 'observe');
      setAutoSandboxConfig({
        allowKeys: sandboxStatus.value?.allowKeys ?? [],
        allowVids: sandboxStatus.value?.allowVids ?? [],
        actOnHid: !!sandboxStatus.value?.actOnHid,
      });
    }
    if (sandboxRecent.status === 'fulfilled') setAutoActions(Array.isArray(sandboxRecent.value) ? sandboxRecent.value : []);

    const scorePairs = await Promise.all(visibleEntries.map(async (entry) => {
      try {
        const score = await invoke<UsbTrustScore>("usb_device_trust_score", { deviceKey: entry.key });
        return [entry.key, score] as const;
      } catch {
        return null;
      }
    }));
    const nextScores: Record<string, UsbTrustScore> = {};
    scorePairs.forEach((pair) => { if (pair) nextScores[pair[0]] = pair[1]; });
    setTrustScores(nextScores);
  }, [advancedAvailable]);

  const refresh = useCallback(async () => {
    setError(null);
    try {
      const [nextStatus, timeline] = await Promise.all([
        invoke<UsbMonitorStatus>('usb_monitor_status'),
        invoke<UsbTimeline>('get_usb_timeline'),
      ]);
      setStatus(nextStatus);
      const startedAt = nextStatus.monitorStartedAt ?? timeline.monitorStartedAt ?? null;
      setMonitorStartedAt(startedAt && startedAt > 0 ? startedAt : null);

      const explicitCurrentKeys = Array.isArray(timeline.currentKeys)
        ? new Set(timeline.currentKeys)
        : new Set(
          nextStatus.running
            ? (timeline.sessions ?? []).filter((row) => row.detachedAt == null && row.endedUnobservedAt == null).map((row) => row.deviceKey)
            : [],
        );
      setCurrentKeys(explicitCurrentKeys);
      setSessions(Array.isArray(timeline.sessions) ? timeline.sessions : []);

      const openRows = new Map<string, UsbSessionRow>();
      for (const row of timeline.sessions ?? []) {
        if (explicitCurrentKeys.has(row.deviceKey) && row.detachedAt == null && row.endedUnobservedAt == null) {
          openRows.set(row.deviceKey, row);
        }
      }

      const visibleEntries = Object.values(timeline.records ?? {})
        .filter((record) => !isInternalUsbPlumbing(record.identity))
        .map((record): UsbTimelineEntry => {
          const open = openRows.get(record.identity.key);
          return {
            key: record.identity.key,
            instanceId: record.identity.instanceId ?? '',
            friendlyName: safeFriendlyName(record.identity),
            category: categoryFor(record.identity),
            lastSeen: record.lastSeen,
            totalPluggedSecs: record.totalPluggedSecs,
            sessionCount: record.sessionCount,
            attached: explicitCurrentKeys.has(record.identity.key),
            driveLetter: open?.volumeLetter ?? null,
            openSinceEpoch: open?.attachedAt ?? null,
          };
        })
        .sort((a, b) => b.lastSeen - a.lastSeen);
      setEntries(visibleEntries);
      void refreshVolumes();
      void refreshAdvanced(visibleEntries);
    } catch (reason) {
      const message = humanizeUsbError(reason);
      setError(message);
      recordUsbFailure('refresh', 'USB.MONITOR.REFRESH_FAILED');
    }
  }, [recordUsbFailure, refreshAdvanced, refreshVolumes]);

  useEffect(() => { void refresh(); }, [refresh]);

  useEffect(() => {
    invoke<{ installed?: boolean }>('get_pro_install_status')
      .then((result) => setProInstalled(!!result?.installed))
      .catch(() => setProInstalled(false));
  }, []);

  useEffect(() => {
    if (!status.running) return;
    const id = window.setInterval(() => setNowSec(Math.floor(Date.now() / 1000)), 1000);
    return () => window.clearInterval(id);
  }, [status.running]);

  useEffect(() => {
    let unmounted = false;
    const attachPromise = listen('usb-device-attached', () => { if (!unmounted) void refresh(); });
    const detachPromise = listen('usb-device-detached', () => { if (!unmounted) void refresh(); });
    return () => {
      unmounted = true;
      void attachPromise.then((unlisten) => unlisten());
      void detachPromise.then((unlisten) => unlisten());
    };
  }, [refresh]);

  useEffect(() => {
    if (!advancedAvailable) return;
    let unmounted = false;
    const alertPromise = listen<HidInjectionAlert>('usb-hid-injection', (event) => {
      if (!unmounted) setHidAlerts((current) => [event.payload, ...current].slice(0, 50));
    });
    const actionPromise = listen<AutoActionRecord>('usb-autosandbox-action', (event) => {
      if (!unmounted) setAutoActions((current) => [event.payload, ...current].slice(0, 50));
    });
    return () => {
      unmounted = true;
      void alertPromise.then((unlisten) => unlisten());
      void actionPromise.then((unlisten) => unlisten());
    };
  }, [advancedAvailable]);

  const timelineEvents = useMemo<TimelineEvent[]>(() => {
    const byKey = new Map(entries.map((entry) => [entry.key, entry]));
    const events: TimelineEvent[] = [];
    for (const session of sessions) {
      const entry = byKey.get(session.deviceKey);
      if (!entry) continue;
      const sourceFor = (time: number): TimelineSource => monitorStartedAt != null && time >= monitorStartedAt
        ? 'Current monitor run'
        : 'Persisted monitor record';
      const connected = currentKeys.has(session.deviceKey)
        && session.detachedAt == null
        && session.endedUnobservedAt == null;
      const attachState: TimelineState = session.detachedAt == null && !connected
        ? 'State unknown'
        : connected
          ? 'Connected now'
          : session.attachedAtEstimated
            ? 'Present when armed'
            : 'Attached';
      events.push({
        id: `${session.deviceKey}:attach:${session.attachedAt}`,
        deviceKey: session.deviceKey,
        name: entry.friendlyName,
        category: entry.category,
        state: attachState,
        at: session.attachedAt,
        durationSecs: connected ? Math.max(0, nowSec - session.attachedAt) : null,
        source: sourceFor(session.attachedAt),
      });
      if (session.detachedAt != null) {
        events.push({
          id: `${session.deviceKey}:detach:${session.detachedAt}`,
          deviceKey: session.deviceKey,
          name: entry.friendlyName,
          category: entry.category,
          state: 'Detached',
          at: session.detachedAt,
          durationSecs: session.durationSecs ?? Math.max(0, session.detachedAt - session.attachedAt),
          source: sourceFor(session.detachedAt),
        });
      } else if (session.endedUnobservedAt != null) {
        events.push({
          id: `${session.deviceKey}:unknown:${session.endedUnobservedAt}`,
          deviceKey: session.deviceKey,
          name: entry.friendlyName,
          category: entry.category,
          state: 'State unknown',
          at: session.endedUnobservedAt,
          durationSecs: null,
          source: 'Persisted monitor record',
        });
      }
    }
    return events.sort((a, b) => b.at - a.at);
  }, [currentKeys, entries, monitorStartedAt, nowSec, sessions]);

  const toggleMonitor = useCallback(async (on: boolean) => {
    setBusy(true);
    setError(null);
    try {
      await invoke(on ? 'start_usb_monitor' : 'stop_usb_monitor');
      await patchAppSettings({ ideal: { privacy: { usbSecurity: { monitorEnabled: on } } } }).catch(reportSettingsWriteFailure);
      await refresh();
    } catch (reason) {
      const message = humanizeUsbError(reason);
      setError(message);
      recordUsbFailure('monitor', 'USB.MONITOR.CONFIG_FAILED');
    } finally {
      setBusy(false);
    }
  }, [patchAppSettings, recordUsbFailure, refresh]);

  const toggleNotify = useCallback(async (on: boolean) => {
    setBusy(true);
    try {
      await invoke('set_usb_monitor_notify', { enabled: on });
      setStatus((current) => ({ ...current, notify: on }));
    } catch (reason) {
      setError(humanizeUsbError(reason));
      recordUsbFailure('set_notification', 'USB.NOTIFICATION.CONFIG_FAILED');
    } finally {
      setBusy(false);
    }
  }, [recordUsbFailure]);

  const clearTimeline = useCallback(async () => {
    const accepted = await requestConfirm({
      title: 'Clear the USB device timeline?',
      description: 'This permanently removes stored USB monitor records. It does not erase Windows system traces or prove that a device was never used.',
      confirmLabel: 'Clear USB history',
    });
    if (!accepted) return;
    setBusy(true);
    try {
      await invoke('clear_usb_timeline');
      await refresh();
    } catch (reason) {
      setError(humanizeUsbError(reason));
    } finally {
      setBusy(false);
    }
  }, [refresh, requestConfirm]);

  const toggleMetering = useCallback(async (on: boolean) => {
    setBusy(true);
    setError(null);
    try {
      if (on) await invoke('start_usb_monitor');
      await invoke(on ? 'start_usb_metering' : 'stop_usb_metering');
      await patchAppSettings({ ideal: { privacy: { usbSecurity: { meteringEnabled: on } } } }).catch(reportSettingsWriteFailure);
      setMetering(on);
      await refresh();
    } catch (reason) {
      setError(humanizeUsbError(reason));
      recordUsbFailure('metering', 'USB.METERING.CONFIG_FAILED');
    } finally {
      setBusy(false);
    }
  }, [patchAppSettings, recordUsbFailure, refresh]);

  const toggleHidGuard = useCallback(async (on: boolean) => {
    setBusy(true);
    setError(null);
    try {
      if (on) await invoke('start_usb_monitor');
      await invoke(on ? 'start_usb_hid_guard' : 'stop_usb_hid_guard');
      await patchAppSettings({ ideal: { privacy: { usbSecurity: { hidGuardEnabled: on } } } }).catch(reportSettingsWriteFailure);
      setHidGuardRunning(on);
      await refresh();
    } catch (reason) {
      setError(humanizeUsbError(reason));
      recordUsbFailure('hid_guard', 'USB.HID_GUARD.CONFIG_FAILED');
    } finally {
      setBusy(false);
    }
  }, [patchAppSettings, recordUsbFailure, refresh]);

  const clearHidAlerts = useCallback(async () => {
    const accepted = await requestConfirm({
      title: 'Clear recent USB HID alerts?',
      description: 'This removes only timing-anomaly metadata. WinCommander never stores the keys that were typed.',
      confirmLabel: 'Clear alerts',
    });
    if (!accepted) return;
    setBusy(true);
    try {
      await invoke('clear_usb_hid_alerts');
      setHidAlerts([]);
    } catch (reason) {
      setError(humanizeUsbError(reason));
    } finally {
      setBusy(false);
    }
  }, [requestConfirm]);

  const setHidSensitivityCmd = useCallback(async (sensitivity: 'lenient' | 'balanced' | 'strict') => {
    setBusy(true);
    try {
      const next = await invoke<{ humanFloorMs: number; minBurstKeys: number }>('set_usb_hid_guard_sensitivity', { sensitivity });
      setHidSensitivity(sensitivity);
      setHidThresholds(next);
    } catch (reason) {
      setError(humanizeUsbError(reason));
    } finally {
      setBusy(false);
    }
  }, []);

  const blockDevice = useCallback(async (entry: UsbTimelineEntry) => {
    const name = displayNameForEntry(entry, volumeForEntry(entry, volumes));
    const accepted = await requestConfirm({
      title: `Disable “${name}”?`,
      description: 'The device will stop working in Windows until you allow it again. This requires WinCommander Pro and administrator rights.',
      confirmLabel: 'Disable device',
    });
    if (!accepted) return;
    setBusy(true);
    try {
      await invoke('block_usb_device', { args: { instanceId: entry.instanceId || entry.key } });
      setBlockedKeys((current) => new Set(current).add(entry.key));
      void showSuccess(`Blocked "${name}" — now disabled in Windows.`);
    } catch (reason) {
      recordUsbFailure('block', 'USB.BLOCK.FAILED');
      const message = humanizeUsbError(reason);
      setError(message);
      void showError(`Block failed: ${message}`);
    } finally {
      setBusy(false);
    }
  }, [recordUsbFailure, requestConfirm, volumes]);

  const allowDevice = useCallback(async (entry: UsbTimelineEntry) => {
    const name = displayNameForEntry(entry, volumeForEntry(entry, volumes));
    if (hidApprovalGateEnabled && entry.category === 'Keyboard / HID') {
      recordUsbFailure('allow', 'USB.HID_APPROVAL.REQUIRED', false);
      void showError(`Use the New keyboard approval dialog for "${name}". Generic Allow is disabled while the approval gate is active.`);
      return;
    }
    setBusy(true);
    try {
      await invoke('allow_usb_device', { args: { instanceId: entry.instanceId || entry.key } });
      setBlockedKeys((current) => {
        const next = new Set(current);
        next.delete(entry.key);
        return next;
      });
      void showSuccess(`Allowed "${name}" — re-enabled in Windows.`);
    } catch (reason) {
      recordUsbFailure('allow', 'USB.ALLOW.FAILED');
      const message = humanizeUsbError(reason);
      setError(message);
      void showError(`Allow failed: ${message}`);
    } finally {
      setBusy(false);
    }
  }, [recordUsbFailure, hidApprovalGateEnabled, volumes]);

  const setVolumeReadonly = useCallback(async (letter: string, readOnly: boolean) => {
    const displayLetter = letter.replace(/:$/, '');
    if (readOnly) {
      const accepted = await requestConfirm({
        title: `Make ${displayLetter}: read-only?`,
        description: 'Writes will be blocked until read-only mode is cleared. Administrator rights are required.',
        confirmLabel: 'Make read-only',
      });
      if (!accepted) return;
    }
    setBusy(true);
    try {
      await invoke('set_usb_volume_readonly', { args: { driveLetter: letter, readOnly } });
      await refresh();
    } catch (reason) {
      setError(humanizeUsbError(reason));
    } finally {
      setBusy(false);
    }
  }, [refresh, requestConfirm]);

  const setHidApprovalGateEnabled = useCallback(async (enabled: boolean) => {
    setHidApprovalBusy(true);
    try {
      if (enabled) await startHidApprovalGate(hidApprovalTtlSecs);
      else await stopHidApprovalGate();
      await patchAppSettings({ ideal: { privacy: { usbSecurity: { hidApprovalGateEnabled: enabled } } } });
    } catch (reason) {
      setError(humanizeUsbError(reason));
    } finally {
      setHidApprovalBusy(false);
    }
  }, [hidApprovalTtlSecs, patchAppSettings, startHidApprovalGate, stopHidApprovalGate]);

  const setHidApprovalTtlSecs = useCallback(async (approvalTtlSecs: number) => {
    setHidApprovalBusy(true);
    try {
      await startHidApprovalGate(approvalTtlSecs);
      await patchAppSettings({ ideal: { privacy: { usbSecurity: { hidApprovalTtlSecs: approvalTtlSecs } } } });
    } catch (reason) {
      setError(humanizeUsbError(reason));
    } finally {
      setHidApprovalBusy(false);
    }
  }, [patchAppSettings, startHidApprovalGate]);

  const toggleAutoSandbox = useCallback(async (on: boolean) => {
    setAutoSandboxBusy(true);
    try {
      if (on) await invoke('start_usb_monitor');
      await invoke(on ? 'start_usb_autosandbox' : 'stop_usb_autosandbox');
      await patchAppSettings({ ideal: { privacy: { usbSecurity: { autoSandboxEnabled: on } } } }).catch(reportSettingsWriteFailure);
      setAutoSandboxRunning(on);
      await refresh();
    } catch (reason) {
      setError(humanizeUsbError(reason));
    } finally {
      setAutoSandboxBusy(false);
    }
  }, [patchAppSettings, refresh]);

  const setAutoSandboxModeCmd = useCallback(async (mode: AutoSandboxMode) => {
    setAutoSandboxBusy(true);
    try {
      const current = await invoke<AutoSandboxStatus>('usb_autosandbox_status').catch(() => null);
      if (mode === 'enforce' && current?.actOnHid) {
        const accepted = await requestConfirm({
          title: 'Enforce automatic keyboard quarantine?',
          description: 'A newly attached untrusted keyboard can be disabled after detection. Continue only if you have tested a recovery path.',
          confirmLabel: 'Enforce with HID scope',
        });
        if (!accepted) return;
      }
      await invoke('set_usb_autosandbox_config', {
        config: {
          mode,
          allowKeys: current?.allowKeys ?? [],
          allowVids: current?.allowVids ?? [],
          actOnHid: current?.actOnHid ?? false,
        },
      });
      setAutoSandboxMode(mode);
      setAutoSandboxConfig({
        allowKeys: current?.allowKeys ?? [],
        allowVids: current?.allowVids ?? [],
        actOnHid: current?.actOnHid ?? false,
      });
    } catch (reason) {
      setError(humanizeUsbError(reason));
    } finally {
      setAutoSandboxBusy(false);
    }
  }, [requestConfirm]);

  const setAutoSandboxHidScope = useCallback(async (actOnHid: boolean) => {
    if (actOnHid && autoSandboxMode === 'enforce') {
      const accepted = await requestConfirm({
        title: 'Also auto-quarantine newly attached keyboards?',
        description: 'In Enforce mode an untrusted keyboard can be disabled after it attaches. Keep this off unless you have tested recovery.',
        confirmLabel: 'Include HID devices',
      });
      if (!accepted) return;
    }
    setAutoSandboxBusy(true);
    try {
      await invoke('set_usb_autosandbox_config', { config: { mode: autoSandboxMode, ...autoSandboxConfig, actOnHid } });
      setAutoSandboxConfig((current) => ({ ...current, actOnHid }));
    } catch (reason) {
      setError(humanizeUsbError(reason));
    } finally {
      setAutoSandboxBusy(false);
    }
  }, [autoSandboxConfig, autoSandboxMode, requestConfirm]);

  const clearAutoActions = useCallback(async () => {
    const accepted = await requestConfirm({
      title: 'Clear recent USB auto-isolate actions?',
      description: 'This removes the recent automatic USB response records. It does not change the current protection policy.',
      confirmLabel: 'Clear actions',
    });
    if (!accepted) return;
    setAutoSandboxBusy(true);
    try {
      await invoke('clear_usb_autosandbox_recent');
      setAutoActions([]);
    } catch (reason) {
      setError(humanizeUsbError(reason));
    } finally {
      setAutoSandboxBusy(false);
    }
  }, [requestConfirm]);

  const connectedCount = entries.filter((entry) => entry.attached).length;
  const alertsToday = advancedAvailable
    ? hidAlerts.filter((alert) => isToday(alert.detectedAt)).length
      + autoActions.filter((action) => action.action !== 'ignore' && isToday(action.time)).length
    : null;
  const lastEvent = timelineEvents[0] ?? null;
  const monitorFailure = status.lastError ?? null;

  const headerRight = (
    <Tag minimal intent={status.running ? (monitorFailure ? 'warning' : 'success') : 'none'} className="font-mono">
      {status.running ? 'ARMED' : 'OFF'}
    </Tag>
  );

  return (
    <SectionCard title="USB Protection" icon="usb" headerRight={headerRight}>
      <div className="flex flex-col gap-4">
        <div className="grid grid-cols-2 gap-2 md:grid-cols-4">
          <div className="rounded-md border border-white/10 p-3">
            <div className="text-xs opacity-60">State</div>
            <div className="mt-1 font-mono text-sm font-semibold">{status.running ? 'Armed' : 'Off'}</div>
          </div>
          <div className="rounded-md border border-white/10 p-3">
            <div className="text-xs opacity-60">Devices currently connected</div>
            <div className="mt-1 font-mono text-sm font-semibold">{status.running ? (monitorFailure ? 'Unknown' : connectedCount) : '—'}</div>
          </div>
          <div className="rounded-md border border-white/10 p-3">
            <div className="text-xs opacity-60">Alerts today</div>
            <div className="mt-1 font-mono text-sm font-semibold">{alertsToday ?? 'N/A'}</div>
          </div>
          <div className="rounded-md border border-white/10 p-3">
            <div className="text-xs opacity-60">Last USB event</div>
            <div className="mt-1 text-sm font-medium">{lastEvent ? safeDate(lastEvent.at) : 'None observed'}</div>
          </div>
        </div>

        <div className="flex items-center gap-2 text-sm opacity-75">
          <span>Monitors USB attach/detach state and optional protection signals; it does not inspect files or typed content.</span>
          <InfoButton label="What USB Protection records">
            The basic monitor stores device presence, a safe Windows-supplied label, category, event time, and observed session duration. It does not log filenames, copied content, keystrokes, or claim activity from periods when monitoring was off.
          </InfoButton>
        </div>

        {monitorFailure && (
          <div role="alert" className="rounded-md border border-[var(--color-warning)]/35 p-3 text-sm">
            <div className="font-semibold">USB watcher needs attention</div>
            <div className="mt-1">{monitorFailure.message}</div>
            <div className="mt-1 text-xs opacity-70">Recovery: {monitorFailure.recoveryAction}</div>
          </div>
        )}
        {error && <div role="alert" className="font-mono text-sm text-[var(--color-danger)]">{error}</div>}

        <section aria-labelledby="usb-timeline-heading" className="flex flex-col gap-2">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex items-center gap-2">
              <h3 id="usb-timeline-heading" className="text-sm font-semibold">Timeline</h3>
              <InfoButton label="About USB timeline coverage">
                {status.windowsHistoryAvailable
                  ? 'Windows historical USB traces are available from the current event source and are labelled separately.'
                  : 'This monitor does not import complete Windows USB history. Stored rows are WinCommander monitor records only, so periods before it was armed remain unknown.'}
              </InfoButton>
            </div>
            <div className="flex flex-wrap gap-2 text-[11px]">
              <Tag minimal intent="success">Connected now</Tag>
              <Tag minimal>Persisted monitor records</Tag>
              <Tag minimal intent="warning">Unknown gaps stay unknown</Tag>
            </div>
          </div>

          {!status.running && (
            <div className="rounded-md border border-white/10 p-3 text-sm opacity-75">
              Monitoring is off. Stored records can be reviewed, but WinCommander cannot say what USB activity occurred while protection was off.
            </div>
          )}

          {status.running && !monitorFailure && timelineEvents.length === 0 && (
            <div className="rounded-md border border-white/10 p-3 text-sm opacity-75">
              No USB events have been observed since monitoring started. This does not prove that no USB devices were used before it was armed.
            </div>
          )}

          {timelineEvents.length > 0 && (
            <PrivacyEventTable
              title="USB attach and detach timeline"
              columns={['Time', 'Device', 'Category', 'State', 'Duration', 'Record source']}
              rows={timelineEvents.map((event) => ({
                id: event.id,
                search: `${event.name} ${event.category} ${event.state} ${event.source}`,
                sort: [String(event.at), event.name, event.category, event.state, String(event.durationSecs ?? -1), event.source],
                cells: [
                  safeDate(event.at),
                  event.name,
                  event.category,
                  <Tag key={`${event.id}-state`} minimal intent={stateIntent(event.state)}>{event.state}</Tag>,
                  formatDuration(event.durationSecs),
                  event.source,
                ],
              }))}
            />
          )}
        </section>

        <section aria-labelledby="usb-trusted-heading" className="flex flex-col gap-2 border-t border-white/10 pt-3">
          <div className="flex items-center gap-2">
            <h3 id="usb-trusted-heading" className="text-sm font-semibold">Trusted devices</h3>
            <InfoButton label="How trusted device status is determined">
              Basic monitoring does not assign trust. With Pro, a numeric trust signal and saved policy exceptions can be shown; these are decision aids, not proof that a device is safe.
            </InfoButton>
          </div>
          {entries.length === 0 ? (
            <div className="text-sm opacity-60">No monitored USB devices are available to review.</div>
          ) : (
            <div className="grid gap-2 md:grid-cols-2">
              {entries.map((entry) => {
                const score = trustScores[entry.key];
                const policyTrusted = autoSandboxConfig.allowKeys.includes(entry.key);
                return (
                  <div key={entry.key} className="rounded-md border border-white/10 p-3">
                    <div className="flex items-start justify-between gap-2">
                      <div>
                        <div className="text-sm font-medium">{displayNameForEntry(entry, volumeForEntry(entry, volumes))}</div>
                        <div className="mt-1 text-xs opacity-60">{entry.category} · {entry.sessionCount} monitored session{entry.sessionCount === 1 ? '' : 's'}</div>
                      </div>
                      <div className="flex flex-wrap justify-end gap-1">
                        {entry.attached && <Tag minimal intent="success">CONNECTED</Tag>}
                        {policyTrusted && <Tag minimal intent="success">TRUSTED BY POLICY</Tag>}
                        {score && (
                          <Tag minimal intent={trustScoreTone(score.score)} className="font-mono">
                            Trust score {formatTrustScore(score.score)}
                          </Tag>
                        )}
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </section>

        <section aria-labelledby="usb-actions-heading" className="flex flex-col gap-3 border-t border-white/10 pt-3">
          <div className="flex items-center gap-2">
            <h3 id="usb-actions-heading" className="text-sm font-semibold">Protection actions</h3>
            <InfoButton label="About USB protection actions">
              Arming starts observation from that point forward. Notifications report attach/detach changes. Clearing the timeline deletes WinCommander records only; it does not change Windows history.
            </InfoButton>
          </div>
          <div className="flex flex-wrap items-center gap-3">
            <Switch
              checked={status.running}
              disabled={busy}
              onChange={(event) => void toggleMonitor((event.target as HTMLInputElement).checked)}
              label="Arm USB Protection"
            />
            <Switch
              checked={status.notify}
              disabled={busy || !status.running}
              onChange={(event) => void toggleNotify((event.target as HTMLInputElement).checked)}
              label="Notify on USB changes"
            />
            <Button icon="refresh" minimal small onClick={() => void refresh()} disabled={busy} aria-label="Refresh USB device timeline">
              Refresh
            </Button>
            <Button icon="trash" minimal small onClick={() => void clearTimeline()} disabled={busy || timelineEvents.length === 0} aria-label="Clear USB device timeline">
              Clear stored records
            </Button>
            {busy && <Spinner size={14} />}
          </div>
          <div className="text-xs opacity-65">
            Clearing records is permanent for WinCommander’s local USB timeline. It does not erase Windows artefacts and must not be used as evidence that a device was never connected.
          </div>
        </section>

        <details className="rounded-md border border-white/10 p-3">
          <summary className="cursor-pointer text-sm font-semibold">Transfer monitoring</summary>
          <div className="mt-3 flex flex-col gap-2">
            <TierGate tier="paid" featureLabel="USB transfer metering" fallback={<div className="text-xs opacity-60">WinCommander Pro is required for aggregate transfer metering.</div>}>
              <Switch
                checked={metering}
                disabled={busy || !status.running}
                onChange={(event) => void toggleMetering((event.target as HTMLInputElement).checked)}
                label="Meter aggregate USB transfer volume"
              />
              <div className="text-xs opacity-60">
                Aggregate byte counts only. Filenames and copied content are not collected.
              </div>
              {stats.length > 0 && (
                <div className="grid gap-1 text-xs">
                  {stats.map((stat) => (
                    <div key={stat.deviceKey}>{stat.friendlyName || 'USB device'}: ↓ {formatBytes(stat.readBytes)} · ↑ {formatBytes(stat.writeBytes)}</div>
                  ))}
                </div>
              )}
            </TierGate>
          </div>
        </details>

        <TierGate
          tier="paid"
          featureLabel="USB HID anomaly alerts and auto-isolate"
          fallback={<div className="text-xs opacity-60">Pro adds USB HID safety, automatic isolation, and enforcement policy.</div>}
        >
          <div className="flex flex-col gap-3">
            <details className="rounded-md border border-white/10 p-3">
              <summary className="cursor-pointer text-sm font-semibold">Keyboard/HID safety</summary>
              <div className="mt-3 flex flex-col gap-3">
                <Switch
                  checked={hidGuardRunning}
                  disabled={busy}
                  onChange={(event) => void toggleHidGuard((event.target as HTMLInputElement).checked)}
                  label="Alert on abnormal USB keyboard timing"
                />
                <div className="text-xs opacity-60">
                  This is a low-confidence timing correlation. The Windows hook cannot prove which keyboard generated input. Keystroke content is never read or logged.
                </div>
                <div className="flex flex-wrap items-center gap-2 text-xs" role="group" aria-label="USB HID timing sensitivity">
                  {(['lenient', 'balanced', 'strict'] as const).map((preset) => (
                    <Button key={preset} small minimal={hidSensitivity !== preset} aria-pressed={hidSensitivity === preset} disabled={busy} onClick={() => void setHidSensitivityCmd(preset)}>
                      {preset.charAt(0).toUpperCase() + preset.slice(1)}
                    </Button>
                  ))}
                  <span className="opacity-50">{hidThresholds.minBurstKeys}+ timing gaps under {hidThresholds.humanFloorMs}ms</span>
                </div>
                {hidAlerts.length > 0 && (
                  <Button icon="trash" minimal small onClick={() => void clearHidAlerts()} disabled={busy} aria-label="Clear USB HID timing alerts">
                    Clear HID alerts
                  </Button>
                )}
                <UsbHidApprovalGateSettings
                  enabled={hidApprovalGateEnabled}
                  ttlSecs={hidApprovalTtlSecs}
                  status={hidApprovalStatus}
                  busy={hidApprovalBusy || !advancedAvailable || !proInstalled}
                  onEnabledChange={(enabled) => void setHidApprovalGateEnabled(enabled)}
                  onTtlChange={(ttlSecs) => void setHidApprovalTtlSecs(ttlSecs)}
                />
              </div>
            </details>

            <details className="rounded-md border border-white/10 p-3">
              <summary className="cursor-pointer text-sm font-semibold">Auto-isolate</summary>
              <div className="mt-3 flex flex-col gap-3">
                <Switch
                  checked={autoSandboxRunning}
                  disabled={autoSandboxBusy}
                  onChange={(event) => void toggleAutoSandbox((event.target as HTMLInputElement).checked)}
                  label="Enable auto-isolate"
                />
                <div className="text-xs opacity-60">
                  Observe alerts only. Enforce can quarantine removable storage after detection. HID enforcement is off by default because disabling a keyboard can lock out input.
                </div>
                <div className="flex flex-wrap gap-2" role="group" aria-label="USB auto-isolate mode">
                  {(['off', 'observe', 'enforce'] as AutoSandboxMode[]).map((m) => (
                    <Button
                      key={m}
                      small
                      minimal={autoSandboxMode !== m}
                      intent={m === 'enforce' && autoSandboxMode === m ? 'danger' : undefined}
                      aria-pressed={autoSandboxMode === m}
                      disabled={autoSandboxBusy}
                      onClick={() => void setAutoSandboxModeCmd(m)}
                    >
                      {m.charAt(0).toUpperCase() + m.slice(1)}
                    </Button>
                  ))}
                </div>
                <Switch
                  checked={autoSandboxConfig.actOnHid}
                  disabled={autoSandboxBusy}
                  onChange={(event) => void setAutoSandboxHidScope((event.target as HTMLInputElement).checked)}
                  label="Include newly attached HID keyboards"
                />
                {autoActions.length > 0 && (
                  <Button icon="trash" minimal small onClick={() => void clearAutoActions()} disabled={autoSandboxBusy} aria-label="Clear USB auto-isolate actions">
                    Clear auto-isolate actions
                  </Button>
                )}
              </div>
            </details>

            <details className="rounded-md border border-white/10 p-3">
              <summary className="cursor-pointer text-sm font-semibold">Advanced policy</summary>
              <div className="mt-3 flex flex-col gap-3">
                <div className="text-xs opacity-60">
                  Device disable/allow and storage read-only enforcement require Pro and administrator rights. Dangerous actions keep their warning visible before confirmation.
                </div>
                {entries.map((entry) => {
                  const name = displayNameForEntry(entry, volumeForEntry(entry, volumes));
                  const volume = volumeForEntry(entry, volumes);
                  const resolvedLetter = volume?.driveLetter ?? entry.driveLetter;
                  const isBlocked = blockedKeys.has(entry.key);
                  const approvalControlledHid = hidApprovalGateEnabled && entry.category === 'Keyboard / HID';
                  return (
                    <div key={`policy-${entry.key}`} className="rounded-md border border-white/10 p-3">
                      <div className="text-sm font-medium">{name}</div>
                      <div className="mt-2 flex flex-wrap gap-1">
                        <Button
                          intent="danger"
                          minimal
                          small
                          aria-label={`Block ${name}`}
                          disabled={busy || isBlocked || !proInstalled || !advancedAvailable}
                          onClick={() => void blockDevice(entry)}
                        >
                          Block
                        </Button>
                        <Button
                          intent="success"
                          minimal
                          small
                          aria-label={`Allow ${name}`}
                          disabled={busy || !proInstalled || !advancedAvailable || approvalControlledHid}
                          onClick={() => void allowDevice(entry)}
                          title={approvalControlledHid ? 'Use the New keyboard approval dialog. Generic Allow cannot bypass its human-presence challenge.' : undefined}
                        >
                          Allow
                        </Button>
                        {entry.category === 'Storage' && (
                          <Button
                            intent="warning"
                            minimal
                            small
                            aria-label={`Make ${name} read-only`}
                            disabled={busy || !resolvedLetter || !advancedAvailable}
                            onClick={() => resolvedLetter && void setVolumeReadonly(resolvedLetter, true)}
                          >
                            Read-only
                          </Button>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            </details>
          </div>
        </TierGate>
      </div>
    </SectionCard>
  );
}
