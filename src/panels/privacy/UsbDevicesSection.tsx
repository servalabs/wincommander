// src/panels/privacy/UsbDevicesSection.tsx
//
// "USB Device Timeline" — U-A foundation: attach/detach timeline for all USB
// devices. Free feature; mutations (start/stop/clear/notify) gated via
// require_paid on the Rust side. No Pro sidecar — runs in-process.
// Later phases (U-B metering, U-C HID-guard, U-D policy) build on the public
// contract: subscribe(), current_devices(), identity_for_key(), is_running().
//
// U-C BadUSB / HID-injection guard is appended below the timeline table.
// Privacy: the guard alert payload carries ONLY timing counts and device
// identity — never any keystroke content.
//
// U-F Auto-isolate subsection is appended after U-C. SAFETY: default mode is
// OBSERVE (alert-only). ENFORCE requires explicit opt-in and acts ONLY on
// removable mass-storage (+ optionally HID). The danger note is mandatory UI.
//
// UI additions:
//   • Per-device risk badge — derived from hidAlerts + autoActions (heuristic,
//     NOT a definitive trust score). High/Medium/Low using existing Tag primitive.
//   • Per-device backend trust score — read-only numeric score from usb_policy.
//   • Summary stat strip — total devices, total plug-time, total data, high-risk
//     count, using existing privacy-stats-strip CSS classes.

import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Button, Spinner, Switch, Tag } from '@/components/ui/bp';
import type { Intent } from '@/components/ui/bp';
import SectionCard from '../../components/shared/SectionCard';
import { formatTrustScore, trustScoreTone } from '../../lib/usbTrust';
import { showSuccess, showError } from '../../utils/toast';

// U-C: shape returned by get_usb_hid_alerts (timing + device identity — no keystroke content).
interface HidInjectionAlert {
  deviceKey: string;
  friendlyName: string;
  detectedAt: string;         // RFC-3339 UTC
  gapsSampled: number;        // distinct keys counted (timing/count only)
  medianGapMs: number;        // median inter-key interval in ms
  recentHidDevice: string | null;
  redFlag: 'hidOnly' | 'composite' | 'unknown';
  severity: 'danger' | 'warning';
}

// Shape returned by get_usb_timeline (Rust must serialize matching camelCase).
interface UsbTimelineEntry {
  key: string;           // stable identity key (vid:pid:serial or fallback)
  instanceId: string;    // raw Windows PnP InstanceId (U-D block/allow target)
  friendlyName: string;  // OS-supplied display name
  vid: string;           // 4-hex vendor ID, e.g. "05ac"
  pid: string;           // 4-hex product ID, e.g. "024f"
  deviceClass: 'HID' | 'Storage' | 'Other'; // USB class label
  lastSeen: string;      // ISO-8601 timestamp of last attach/detach event
  lastSeenRelative: string; // human-relative, e.g. "3 min ago" — pre-formatted by Rust
  totalPluggedSecs: number; // cumulative seconds of FINISHED (detached) sessions
  sessionCount: number;  // number of distinct plug sessions recorded
  attached: boolean;     // true if currently plugged in
  driveLetter: string | null; // mounted volume letter, from the current open session (Storage only)
  openSinceEpoch: number | null; // epoch secs the current open session began, else null
}

// Shape returned by usb_monitor_status.
interface UsbMonitorStatus {
  running: boolean;
  notify: boolean;
}

// Raw shapes returned by get_usb_timeline (camelCase from the Rust structs).
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
  lastSeen: number; // epoch seconds
  totalPluggedSecs: number;
  sessionCount: number;
}
interface UsbSessionRow {
  deviceKey: string;
  attachedAt: number;
  detachedAt: number | null;
  volumeLetter?: string | null;
}
interface UsbTimeline {
  records: Record<string, UsbDeviceRecord>;
  sessions: UsbSessionRow[];
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

function formatRelative(epochSecs: number): string {
  const diff = Math.max(0, Math.floor(Date.now() / 1000) - epochSecs);
  if (diff < 60) return `${diff}s ago`;
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return `${Math.floor(diff / 86400)}d ago`;
}

function classIntent(cls: UsbTimelineEntry['deviceClass']): Intent | undefined {
  if (cls === 'HID') return 'warning';
  if (cls === 'Storage') return 'danger';
  return undefined;
}

function formatTotalTime(secs: number): string {
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m`;
  return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
}

// Total time the device has been plugged in: finished sessions PLUS, for a device
// that is still attached, the live elapsed time of its current open session.
// `totalPluggedSecs` alone only counts detached sessions, so a still-connected
// device otherwise reads 0 until it's unplugged.
function livePluggedSecs(entry: UsbTimelineEntry, nowSec: number): number {
  const live =
    entry.attached && entry.openSinceEpoch != null
      ? Math.max(0, nowSec - entry.openSinceEpoch)
      : 0;
  return entry.totalPluggedSecs + live;
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(2)} GB`;
}

// Heuristic risk level derived purely from already-fetched HID alerts and
// auto-sandbox actions — NOT a definitive trust score.
type RiskLevel = 'High' | 'Medium' | 'Low';

function riskLevel(
  entry: UsbTimelineEntry,
  hidAlerts: HidInjectionAlert[],
  autoActions: AutoActionRecord[],
): RiskLevel {
  const hasHidAlert = hidAlerts.some((a) => a.deviceKey === entry.key);
  const hasQuarantine = autoActions.some(
    (a) => a.deviceKey === entry.key && a.action === 'quarantine',
  );
  if (entry.deviceClass === 'Storage' && (hasHidAlert || hasQuarantine)) return 'High';
  if (entry.deviceClass === 'HID' && hasHidAlert) return 'High';
  if (entry.deviceClass === 'Storage') return 'Medium';
  return 'Low';
}

function riskIntent(level: RiskLevel): Intent {
  if (level === 'High') return 'danger';
  if (level === 'Medium') return 'warning';
  return 'success';
}

// U-B data-transfer metering — one row per actively-metered device.
interface UsbTransferStat {
  deviceKey: string;
  friendlyName: string;
  readBytes: number;
  writeBytes: number;
  lastSampleEpoch: number;
}

// USB-attached storage volume, as File Explorer presents it. Returned by
// get_usb_storage_volumes (camelCase from the Rust UsbVolume struct).
interface UsbVolume {
  driveLetter: string; // "E:"
  label: string;       // volume label (Explorer's name); may be empty
  model: string;       // disk model, e.g. "SanDisk Ultra USB Device"
  serial: string;      // USB disk serial (uppercased) for device-key correlation
}

// Correlate a Storage timeline entry to one of the mounted USB volumes, mirroring
// the backend's usb_monitor::correlate_volume_letter: match by the serial embedded
// in the device key, else fall back to the sole volume when only one is mounted.
function volumeForEntry(
  entry: UsbTimelineEntry,
  volumes: UsbVolume[],
): UsbVolume | undefined {
  if (entry.deviceClass !== 'Storage' || volumes.length === 0) return undefined;
  const serial = (entry.key.split(':')[3] ?? '').toUpperCase();
  if (serial && serial !== 'NOSERIAL') {
    const bySerial = volumes.find((v) => {
      const vs = (v.serial || '').toUpperCase();
      return vs !== '' && (vs === serial || vs.includes(serial) || serial.includes(vs));
    });
    if (bySerial) return bySerial;
  }
  if (volumes.length === 1) return volumes[0];
  return undefined;
}

// File-Explorer-style display name for a storage device: "Label (E:)", falling
// back to the disk model, then to the raw PnP friendly name when no volume is
// resolved (unformatted / no-media / ambiguous multi-volume host).
function displayNameForEntry(entry: UsbTimelineEntry, vol: UsbVolume | undefined): string {
  if (vol) {
    const base = vol.label || vol.model || 'USB Drive';
    return `${base} (${vol.driveLetter})`;
  }
  return entry.friendlyName;
}

// Block/Allow route through the Pro sidecar, which must be installed (a paid
// entitlement alone isn't enough). Translate that backend signal into a clear
// instruction instead of leaking the raw "PRO_NOT_INSTALLED:" marker string.
function humanizeUsbError(e: unknown): string {
  const s = String(e);
  if (s.includes('PRO_NOT_INSTALLED')) {
    return 'This action needs WinCommander Pro installed. Open Settings → Pro to install it.';
  }
  return s.replace(/^Error:\s*/i, '');
}

// U-F: auto-sandbox / quarantine orchestration types.
type AutoSandboxMode = 'off' | 'observe' | 'enforce';

interface AutoSandboxStatus {
  running: boolean;
  mode: AutoSandboxMode;
  recentCount: number;
}

interface AutoActionRecord {
  time: string;            // ISO-8601 or epoch string
  deviceKey: string;
  friendlyName: string;
  action: 'ignore' | 'alert' | 'quarantine';
  enforced: boolean;
  detail: string;
}

export default function UsbDevicesSection() {
  const [running, setRunning] = useState(false);
  const [notifyEnabled, setNotifyEnabled] = useState(false);
  const [entries, setEntries] = useState<UsbTimelineEntry[]>([]);
  const [trustScores, setTrustScores] = useState<Record<string, UsbTrustScore>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [metering, setMetering] = useState(false);
  const [stats, setStats] = useState<UsbTransferStat[]>([]);
  // Mounted USB storage volumes — used to show Explorer-style names + drive letters.
  const [volumes, setVolumes] = useState<UsbVolume[]>([]);
  // U-D: device keys we blocked this session, so Allow has a visible target and
  // we can show a BLOCKED badge. Windows-side block state isn't queryable cheaply,
  // so this tracks intent within the session.
  const [blockedKeys, setBlockedKeys] = useState<Set<string>>(new Set());
  // Block/Allow enforce via the Pro sidecar binary — which must be INSTALLED
  // (a paid entitlement alone isn't enough). Track it so we can disable the
  // buttons and say why, instead of letting them fail silently.
  const [proInstalled, setProInstalled] = useState<boolean>(true);
  // Ticking wall-clock (epoch secs) so "Total plug time" for a still-attached
  // device counts up live instead of sitting at its last-refresh value.
  const [nowSec, setNowSec] = useState<number>(() => Math.floor(Date.now() / 1000));
  // U-C: HID-injection guard state
  const [hidGuardRunning, setHidGuardRunning] = useState(false);
  const [hidAlerts, setHidAlerts] = useState<HidInjectionAlert[]>([]);
  // U-F: auto-sandbox state
  const [autoSandboxRunning, setAutoSandboxRunning] = useState(false);
  const [autoSandboxMode, setAutoSandboxMode] = useState<AutoSandboxMode>('observe');
  const [autoActions, setAutoActions] = useState<AutoActionRecord[]>([]);
  const [autoSandboxBusy, setAutoSandboxBusy] = useState(false);

  // Fetch mounted USB volumes (Explorer-style names + drive letters). Isolated
  // from refresh() because it shells out to a potentially-slow PowerShell query;
  // it must never block the timeline render or a toggle's busy state. The backend
  // caps the query with a hard timeout, so worst case this resolves to [].
  const refreshVolumes = useCallback(async () => {
    try {
      const vols = await invoke<UsbVolume[]>('get_usb_storage_volumes');
      setVolumes(Array.isArray(vols) ? vols : []);
    } catch {
      setVolumes([]);
    }
  }, []);

  const refresh = useCallback(async () => {
    setError(null);
    try {
      const status = await invoke<UsbMonitorStatus>('usb_monitor_status');
      setRunning(!!status?.running);
      setNotifyEnabled(!!status?.notify);
      const timeline = await invoke<UsbTimeline>('get_usb_timeline');
      const attached = new Set(
        (timeline?.sessions ?? [])
          .filter((s) => s.detachedAt == null)
          .map((s) => s.deviceKey),
      );
      // Drive letter comes from the device's current OPEN session only — a
      // detached session's volume letter no longer identifies a mounted
      // volume, so it must not be offered to set_usb_volume_readonly.
      const openDriveLetters = new Map<string, string | null>();
      const openSince = new Map<string, number>();
      (timeline?.sessions ?? [])
        .filter((s) => s.detachedAt == null)
        .forEach((s) => {
          openDriveLetters.set(s.deviceKey, s.volumeLetter ?? null);
          if (typeof s.attachedAt === 'number') openSince.set(s.deviceKey, s.attachedAt);
        });
      const rows: UsbTimelineEntry[] = Object.values(timeline?.records ?? {})
        .map((r): UsbTimelineEntry => ({
          key: r.identity.key,
          instanceId: r.identity.instanceId ?? '',
          friendlyName: r.identity.friendlyName || `USB ${r.identity.vid}:${r.identity.pid}`,
          vid: r.identity.vid,
          pid: r.identity.pid,
          deviceClass: r.identity.isHid ? 'HID' : r.identity.isMassStorage ? 'Storage' : 'Other',
          lastSeen: new Date(r.lastSeen * 1000).toISOString(),
          lastSeenRelative: formatRelative(r.lastSeen),
          totalPluggedSecs: r.totalPluggedSecs,
          sessionCount: r.sessionCount,
          attached: attached.has(r.identity.key),
          driveLetter: openDriveLetters.get(r.identity.key) ?? null,
          openSinceEpoch: openSince.get(r.identity.key) ?? null,
        }))
        // Show only devices the user actually plugs in: storage and input (HID).
        // Everything classified "Other" — root hubs, host controllers, USB hubs,
        // built-in Bluetooth/WiFi radios, composite parents — is internal plumbing
        // and is hidden from the timeline.
        .filter((r) => r.deviceClass === 'Storage' || r.deviceClass === 'HID')
        .sort((a, b) => a.lastSeen.localeCompare(b.lastSeen));
      setEntries(rows);
      // Explorer-style names + drive letters for storage rows. This shells out to
      // PowerShell (WMI/CIM) which can be slow, so it is fired-and-forget — NEVER
      // awaited here. Awaiting it used to wedge the whole refresh (and any toggle
      // that does `await refresh()`) with the busy spinner stuck on.
      void refreshVolumes();
      const scorePairs = await Promise.all(
        rows.map(async (entry) => {
          try {
            const score = await invoke<UsbTrustScore>("usb_device_trust_score", {
              deviceKey: entry.key,
            });
            return [entry.key, score] as const;
          } catch {
            return null;
          }
        }),
      );
      const nextScores: Record<string, UsbTrustScore> = {};
      scorePairs.forEach((pair) => {
        if (pair) nextScores[pair[0]] = pair[1];
      });
      setTrustScores(nextScores);
      const meteringRunning = await invoke<boolean>('usb_metering_status');
      setMetering(!!meteringRunning);
      const ts = await invoke<UsbTransferStat[]>('get_usb_transfer_stats');
      setStats(Array.isArray(ts) ? ts : []);
      // U-C: fetch HID guard status + recent alerts
      const hidStatus = await invoke<{ running: boolean; alertCount: number }>('usb_hid_guard_status');
      setHidGuardRunning(!!hidStatus?.running);
      const alerts = await invoke<HidInjectionAlert[]>('get_usb_hid_alerts');
      setHidAlerts(Array.isArray(alerts) ? alerts : []);
      // U-F: fetch auto-sandbox status + recent actions
      const asSt = await invoke<AutoSandboxStatus>('usb_autosandbox_status');
      setAutoSandboxRunning(!!asSt?.running);
      setAutoSandboxMode(asSt?.mode ?? 'observe');
      const recent = await invoke<AutoActionRecord[]>('get_usb_autosandbox_recent');
      setAutoActions(Array.isArray(recent) ? recent : []);
    } catch (e) {
      setError(String(e));
    }
  }, [refreshVolumes]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Whether the Pro sidecar binary is installed — decides if Block/Allow can work.
  useEffect(() => {
    invoke<{ installed?: boolean }>('get_pro_install_status')
      .then((s) => setProInstalled(!!s?.installed))
      .catch(() => setProInstalled(false));
  }, []);

  // Tick the wall-clock every second while monitoring so live plug time advances.
  useEffect(() => {
    if (!running) return;
    const id = window.setInterval(() => setNowSec(Math.floor(Date.now() / 1000)), 1000);
    return () => window.clearInterval(id);
  }, [running]);

  // Live meter: while metering is active, poll transfer totals so the numbers
  // tick up as data copies. Without this the stats only moved on a manual
  // Refresh or an attach/detach event, so an in-progress copy looked like "0".
  useEffect(() => {
    if (!running || !metering) return;
    const id = window.setInterval(() => {
      invoke<UsbTransferStat[]>('get_usb_transfer_stats')
        .then((ts) => setStats(Array.isArray(ts) ? ts : []))
        .catch(() => { /* best-effort live poll */ });
    }, 3000);
    return () => window.clearInterval(id);
  }, [running, metering]);

  // Keep volume names/letters fresh while a storage device is present: a drive
  // mounts a moment AFTER its device attaches, so the attach-time fetch can miss
  // it. Polled on a slow cadence (the query is heavier than the in-proc reads),
  // only when there's actually a storage device to resolve, with an in-flight
  // guard so a slow query can't stack up.
  const hasStorage = entries.some((e) => e.deviceClass === 'Storage');
  useEffect(() => {
    if (!running || !hasStorage) return;
    let inFlight = false;
    const tick = async () => {
      if (inFlight) return;
      inFlight = true;
      try {
        await refreshVolumes();
      } finally {
        inFlight = false;
      }
    };
    const id = window.setInterval(() => void tick(), 8000);
    return () => window.clearInterval(id);
  }, [running, hasStorage, refreshVolumes]);

  // Live-update: re-fetch timeline on attach/detach events. On attach, also
  // schedule two quick volume re-fetches — Windows mounts the volume a few
  // seconds AFTER the device attaches, so the fetch fired with the attach event
  // is usually too early to see the drive letter / Explorer name.
  useEffect(() => {
    let unmounted = false;
    const timers: number[] = [];
    const attachPromise = listen('usb-device-attached', () => {
      if (unmounted) return;
      void refresh();
      for (const delayMs of [2500, 6000]) {
        timers.push(
          window.setTimeout(() => {
            if (!unmounted) void refreshVolumes();
          }, delayMs),
        );
      }
    });
    const detachPromise = listen('usb-device-detached', () => {
      if (!unmounted) void refresh();
    });
    return () => {
      unmounted = true;
      timers.forEach((t) => window.clearTimeout(t));
      void attachPromise.then((unlisten) => unlisten());
      void detachPromise.then((unlisten) => unlisten());
    };
  }, [refresh, refreshVolumes]);

  // U-C: live-update on HID-injection detection event (timing/device only — no keystroke content).
  useEffect(() => {
    let unmounted = false;
    const injectionPromise = listen<HidInjectionAlert>('usb-hid-injection', (ev) => {
      if (!unmounted) {
        setHidAlerts((prev) => {
          const next = [ev.payload, ...prev].slice(0, 50);
          return next;
        });
      }
    });
    return () => {
      unmounted = true;
      void injectionPromise.then((unlisten) => unlisten());
    };
  }, []);

  // U-F: live-update on auto-sandbox action events.
  useEffect(() => {
    let unmounted = false;
    const actionPromise = listen<AutoActionRecord>('usb-autosandbox-action', (ev) => {
      if (!unmounted) {
        setAutoActions((prev) => {
          const next = [ev.payload, ...prev].slice(0, 50);
          return next;
        });
      }
    });
    return () => {
      unmounted = true;
      void actionPromise.then((unlisten) => unlisten());
    };
  }, []);

  const toggle = useCallback(
    async (on: boolean) => {
      setBusy(true);
      setError(null);
      try {
        await invoke(on ? 'start_usb_monitor' : 'stop_usb_monitor');
        await refresh();
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(false);
      }
    },
    [refresh],
  );

  // U-C: toggle BadUSB / HID-injection guard
  const toggleHidGuard = useCallback(
    async (on: boolean) => {
      setBusy(true);
      setError(null);
      try {
        await invoke(on ? 'start_usb_hid_guard' : 'stop_usb_hid_guard');
        setHidGuardRunning(on);
        await refresh();
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(false);
      }
    },
    [refresh],
  );

  const clearHidAlerts = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      await invoke('clear_usb_hid_alerts');
      setHidAlerts([]);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, []);

  const toggleNotify = useCallback(
    async (on: boolean) => {
      setBusy(true);
      setError(null);
      try {
        await invoke('set_usb_monitor_notify', { enabled: on });
        setNotifyEnabled(on);
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(false);
      }
    },
    [],
  );

  const toggleMetering = useCallback(
    async (on: boolean) => {
      setBusy(true);
      setError(null);
      try {
        await invoke(on ? 'start_usb_metering' : 'stop_usb_metering');
        setMetering(on);
        await refresh();
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(false);
      }
    },
    [refresh],
  );

  // U-D: block / allow a device by its raw Windows InstanceId. U-A now exposes
  // `instanceId` on each timeline entry; we fall back to the device key only for
  // records that predate that field.
  const blockDevice = useCallback(
    async (entry: UsbTimelineEntry) => {
      const name = displayNameForEntry(entry, volumeForEntry(entry, volumes));
      if (
        !window.confirm(
          `Disable "${name}" now?\n\nThe device will stop working in Windows until you Allow it again. Requires WinCommander Pro and admin rights.`,
        )
      ) {
        return;
      }
      setBusy(true);
      setError(null);
      try {
        await invoke('block_usb_device', { args: { instanceId: entry.instanceId || entry.key } });
        setBlockedKeys((prev) => new Set(prev).add(entry.key));
        void showSuccess(`Blocked "${name}" — now disabled in Windows.`);
        await refresh();
      } catch (e) {
        const msg = humanizeUsbError(e);
        setError(msg);
        void showError(`Block failed: ${msg}`);
      } finally {
        setBusy(false);
      }
    },
    [refresh, volumes],
  );

  const allowDevice = useCallback(
    async (entry: UsbTimelineEntry) => {
      const name = displayNameForEntry(entry, volumeForEntry(entry, volumes));
      setBusy(true);
      setError(null);
      try {
        await invoke('allow_usb_device', { args: { instanceId: entry.instanceId || entry.key } });
        setBlockedKeys((prev) => {
          const next = new Set(prev);
          next.delete(entry.key);
          return next;
        });
        void showSuccess(`Allowed "${name}" — re-enabled in Windows.`);
        await refresh();
      } catch (e) {
        const msg = humanizeUsbError(e);
        setError(msg);
        void showError(`Allow failed: ${msg}`);
      } finally {
        setBusy(false);
      }
    },
    [refresh, volumes],
  );

  // U-E: set a mounted storage volume read-only via diskpart.
  // The drive letter comes from UsbSession.volume_letter; the Rust side validates
  // it to a single A-Z char before interpolation.
  const setVolumeReadonly = useCallback(
    async (letter: string, readOnly: boolean) => {
      const displayLetter = letter.replace(/:$/, '');
      if (
        readOnly &&
        !window.confirm(
          `Force volume ${displayLetter}: read-only?\n\nWrites will be blocked until you clear read-only. Requires admin (best-effort on already-mounted volumes — may need a re-plug).`,
        )
      ) {
        return;
      }
      setBusy(true);
      setError(null);
      try {
        await invoke('set_usb_volume_readonly', { args: { driveLetter: letter, readOnly } });
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
    setBusy(true);
    setError(null);
    try {
      await invoke('clear_usb_timeline');
      setEntries([]);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, []);

  // U-F: start / stop auto-sandbox monitor.
  const toggleAutoSandbox = useCallback(
    async (on: boolean) => {
      setAutoSandboxBusy(true);
      setError(null);
      try {
        await invoke(on ? 'start_usb_autosandbox' : 'stop_usb_autosandbox');
        setAutoSandboxRunning(on);
        await refresh();
      } catch (e) {
        setError(String(e));
      } finally {
        setAutoSandboxBusy(false);
      }
    },
    [refresh],
  );

  // U-F: change operating mode (off / observe / enforce).
  const setAutoSandboxModeCmd = useCallback(
    async (mode: AutoSandboxMode) => {
      setAutoSandboxBusy(true);
      setError(null);
      try {
        // Fetch current config, patch mode, send back.
        const current = await invoke<{
          mode: AutoSandboxMode;
          allowKeys: string[];
          allowVids: string[];
          actOnHid: boolean;
        }>('usb_autosandbox_status').catch(() => null);
        // Patch ONLY the mode; preserve the rest of the current config. The
        // previous code wrote `current ? [] : []` (both branches empty), so
        // every mode switch silently wiped the approved-device allow-list and
        // reset actOnHid, causing a trusted device to be treated as untrusted
        // the next time it was attached under 'enforce'.
        await invoke('set_usb_autosandbox_config', {
          config: {
            mode,
            allowKeys: current?.allowKeys ?? [],
            allowVids: current?.allowVids ?? [],
            actOnHid: current?.actOnHid ?? false,
          },
        });
        setAutoSandboxMode(mode);
      } catch (e) {
        setError(String(e));
      } finally {
        setAutoSandboxBusy(false);
      }
    },
    [],
  );

  // U-F: clear recent actions ring.
  const clearAutoActions = useCallback(async () => {
    setAutoSandboxBusy(true);
    setError(null);
    try {
      await invoke('clear_usb_autosandbox_recent');
      setAutoActions([]);
    } catch (e) {
      setError(String(e));
    } finally {
      setAutoSandboxBusy(false);
    }
  }, []);

  const attachedCount = entries.filter((e) => e.attached).length;
  const headerRight = (
    <Tag minimal intent={attachedCount > 0 ? 'primary' : running ? 'success' : 'none'} className="font-mono">
      {attachedCount > 0
        ? `${attachedCount} ATTACHED`
        : running
          ? 'WATCHING'
          : 'OFF'}
    </Tag>
  );

  return (
    <SectionCard title="USB Device Timeline" icon="usb" headerRight={headerRight}>
      <div className="flex flex-col gap-3">
        <div className="text-sm opacity-80">
          Tracks USB device attach and detach events — friendly name, VID/PID, device
          class, session count, and cumulative plug time. HID and Storage devices are
          highlighted.
        </div>

        <div className="flex flex-wrap items-center gap-3">
          <Switch
            checked={running}
            disabled={busy}
            onChange={(e) => toggle((e.target as HTMLInputElement).checked)}
            label="Monitor USB activity"
          />
          <Switch
            checked={notifyEnabled}
            disabled={busy || !running}
            onChange={(e) => toggleNotify((e.target as HTMLInputElement).checked)}
            label="Notify on plug/unplug"
          />
          <Switch
            checked={metering}
            disabled={busy || !running}
            onChange={(e) => toggleMetering((e.target as HTMLInputElement).checked)}
            label="Meter data transfer"
          />
          <Button icon="refresh" minimal small onClick={() => void refresh()} disabled={busy}>
            Refresh
          </Button>
          <Button
            icon="trash"
            minimal
            small
            onClick={clear}
            disabled={busy || entries.length === 0}
          >
            Clear
          </Button>
          {busy && <Spinner size={14} />}
        </div>

        {running && metering && stats.length > 0 && (
          <div className="flex flex-col gap-1 border-t border-white/10 pt-2">
            <div className="text-xs font-semibold opacity-70">Data transferred (this session)</div>
            {stats.map((s) => {
              const statEntry = entries.find((e) => e.key === s.deviceKey);
              const label = statEntry
                ? displayNameForEntry(statEntry, volumeForEntry(statEntry, volumes))
                : s.friendlyName || s.deviceKey;
              return (
                <div key={s.deviceKey} className="font-mono text-xs opacity-70">
                  {label}: &darr; {formatBytes(s.readBytes)} read &middot; &uarr;{' '}
                  {formatBytes(s.writeBytes)} written
                </div>
              );
            })}
          </div>
        )}
        {running && metering && stats.length === 0 && (
          <div className="border-t border-white/10 pt-2 text-xs opacity-60">
            Metering active — plug in or copy to a USB drive and totals will appear here.
            Figures are approximate (all volume I/O, sampled every few seconds).
          </div>
        )}

        {error && <div className="font-mono text-sm text-[var(--color-danger)]">{error}</div>}

        {running && entries.length > 0 && (() => {
          const totalPlugSecs = entries.reduce((s, e) => s + livePluggedSecs(e, nowSec), 0);
          const totalBytes = stats.reduce((s, t) => s + t.readBytes + t.writeBytes, 0);
          const highCount = entries.filter(
            (e) => riskLevel(e, hidAlerts, autoActions) === 'High',
          ).length;
          return (
            <div className="privacy-stats-strip">
              <div className="privacy-stat">
                <span className="privacy-stat-n">{entries.length}</span>
                <span className="privacy-stat-l">Devices</span>
              </div>
              <div className="privacy-stat-divider" />
              <div className="privacy-stat">
                <span className="privacy-stat-n">{formatTotalTime(totalPlugSecs)}</span>
                <span className="privacy-stat-l">Total plug time</span>
              </div>
              {stats.length > 0 && (
                <>
                  <div className="privacy-stat-divider" />
                  <div className="privacy-stat">
                    <span className="privacy-stat-n">{formatBytes(totalBytes)}</span>
                    <span className="privacy-stat-l">Data transferred</span>
                  </div>
                </>
              )}
              <div className="privacy-stat-divider" />
              <div className="privacy-stat">
                <span className={`privacy-stat-n${highCount > 0 ? ' usb-stat-high' : ''}`}>
                  {highCount}
                </span>
                <span className="privacy-stat-l">High risk</span>
              </div>
            </div>
          );
        })()}

        {running && entries.length === 0 && (
          <div className="text-sm opacity-70">No USB device events recorded.</div>
        )}

        {!running && (
          <div className="text-sm opacity-50">Monitoring off — enable to see device history.</div>
        )}

        {running && entries.length > 0 && (
          <div className="flex flex-col gap-2">
            <div className="text-xs opacity-60">
              <strong>Block</strong> disables a device in Windows so it stops working (like Device
              Manager → Disable); <strong>Allow</strong> re-enables a blocked one.
              {!proInstalled && (
                <>
                  {' '}These need <strong>WinCommander Pro</strong> installed — open Settings → Pro to
                  enable them.
                </>
              )}
            </div>
            {entries
              .slice()
              .reverse()
              .map((entry, i) => {
                const vol = volumeForEntry(entry, volumes);
                const name = displayNameForEntry(entry, vol);
                const resolvedLetter = vol?.driveLetter ?? entry.driveLetter;
                const isBlocked = blockedKeys.has(entry.key);
                return (
                <div
                  key={`${entry.key}-${i}`}
                  className="flex items-start gap-2 border-t border-white/10 pt-2"
                >
                  <div className="flex flex-col gap-1 pt-0.5">
                    <Tag minimal={entry.deviceClass === 'Other'} intent={classIntent(entry.deviceClass)} className="font-mono">
                      {entry.deviceClass}
                    </Tag>
                    {(() => {
                      const level = riskLevel(entry, hidAlerts, autoActions);
                      return (
                        <Tag
                          minimal
                          intent={riskIntent(level)}
                          className="font-mono"
                          title="Heuristic risk score derived from HID-injection alerts and auto-sandbox quarantine history — not a definitive trust score."
                        >
                          {level}
                        </Tag>
                      );
                    })()}
                    {(() => {
                      const score = trustScores[entry.key];
                      if (!score) return null;
                      return (
                        <Tag
                          minimal
                          intent={trustScoreTone(score.score)}
                          className="font-mono"
                          title="Trust score combines USB identity stability, vendor signal, HID alerts, quarantine history, and transfer volume."
                        >
                          Trust score {formatTrustScore(score.score)}
                        </Tag>
                      );
                    })()}
                    {entry.attached && (
                      <Tag minimal intent="success" className="font-mono">
                        LIVE
                      </Tag>
                    )}
                    {isBlocked && (
                      <Tag
                        minimal
                        intent="danger"
                        className="font-mono"
                        title="You blocked this device this session — it is disabled in Windows until you Allow it."
                      >
                        BLOCKED
                      </Tag>
                    )}
                  </div>
                  <div className="flex-1">
                    <div className="text-sm font-medium">{name}</div>
                    <div className="font-mono text-xs opacity-60">
                      {entry.vid}:{entry.pid} &middot; last seen {entry.lastSeenRelative}
                    </div>
                    <div className="font-mono text-xs opacity-50">
                      {entry.sessionCount} session{entry.sessionCount === 1 ? '' : 's'} &middot;{' '}
                      {formatTotalTime(livePluggedSecs(entry, nowSec))}
                      {entry.attached ? ' plugged in' : ' total'}
                    </div>
                    {/* U-D: Block / Allow — Block disables the device in Windows so it
                        stops working (like Device Manager → Disable); Allow re-enables a
                        blocked one. Enforced by the Pro sidecar; targets the raw InstanceId. */}
                    <div className="flex items-center gap-1 mt-1">
                      <Button
                        intent="danger"
                        minimal
                        small
                        disabled={busy || isBlocked || !proInstalled}
                        onClick={() => void blockDevice(entry)}
                        title={
                          proInstalled
                            ? 'Disable this device in Windows so it stops working (Device Manager → Disable). Reversible with Allow. Needs admin.'
                            : 'Install WinCommander Pro (Settings → Pro) to disable/enable USB devices.'
                        }
                      >
                        Block
                      </Button>
                      <Button
                        intent="success"
                        minimal
                        small
                        disabled={busy || !proInstalled}
                        onClick={() => void allowDevice(entry)}
                        title={
                          proInstalled
                            ? 'Re-enable this device in Windows if it was blocked/disabled. Needs admin.'
                            : 'Install WinCommander Pro (Settings → Pro) to disable/enable USB devices.'
                        }
                      >
                        Allow
                      </Button>
                      {/* U-E: Read-only toggle — storage only, and only once we've resolved
                          a mounted drive letter. Without one, disable rather than send an
                          empty driveLetter that would silently no-op on the backend. */}
                      {entry.deviceClass === 'Storage' && (
                        <>
                          <Button
                            intent="warning"
                            minimal
                            small
                            disabled={busy || !resolvedLetter}
                            onClick={() =>
                              resolvedLetter && void setVolumeReadonly(resolvedLetter, true)
                            }
                            title={
                              resolvedLetter
                                ? `Force volume ${resolvedLetter} read-only via diskpart (best-effort)`
                                : 'No mounted drive letter resolved for this device yet — re-plug or refresh'
                            }
                          >
                            Read-only
                          </Button>
                        </>
                      )}
                    </div>
                  </div>
                </div>
                );
              })}
          </div>
        )}

        {/* U-C: BadUSB / HID-injection guard */}
        <div className="border-t border-white/10 pt-3 flex flex-col gap-2">
          <div className="flex flex-wrap items-center gap-3">
            <div className="text-sm font-semibold">BadUSB guard</div>
            <Switch
              checked={hidGuardRunning}
              disabled={busy}
              onChange={(e) => toggleHidGuard((e.target as HTMLInputElement).checked)}
              label="Detect HID-injection / Rubber Ducky"
            />
            {hidAlerts.length > 0 && (
              <Button
                icon="trash"
                minimal
                small
                onClick={clearHidAlerts}
                disabled={busy}
              >
                Clear alerts
              </Button>
            )}
            <Tag minimal intent={hidGuardRunning ? 'success' : 'none'} className="font-mono">
              {hidGuardRunning ? 'ACTIVE' : 'OFF'}
            </Tag>
          </div>
          <div className="text-xs opacity-60">
            Detects USB devices that enumerate as a keyboard and type superhumanly fast (BadUSB /
            Rubber Ducky / HID-injection). Timing only — keystroke content is never read or logged.
          </div>

          {hidAlerts.length > 0 && (
            <div className="flex flex-col gap-1 mt-1">
              <div className="text-xs font-semibold opacity-70">Recent injection alerts</div>
              {hidAlerts.map((a, i) => (
                <div
                  key={`${a.deviceKey}-${a.detectedAt}-${i}`}
                  className="flex items-start gap-2 border-t border-white/10 pt-1"
                >
                  <Tag
                    minimal
                    intent={a.severity === 'danger' ? 'danger' : 'warning'}
                    className="font-mono"
                  >
                    {a.redFlag === 'composite' ? 'COMPOSITE' : a.redFlag === 'hidOnly' ? 'HID-ONLY' : 'HID'}
                  </Tag>
                  <div className="flex-1">
                    <div className="text-sm font-medium">{a.friendlyName}</div>
                    <div className="font-mono text-xs opacity-60">
                      {a.gapsSampled} keys &middot; median {a.medianGapMs}ms
                    </div>
                    <div className="font-mono text-xs opacity-50">
                      {new Date(a.detectedAt).toLocaleString()}
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}

          {hidAlerts.length === 0 && hidGuardRunning && (
            <div className="text-xs opacity-50">No injection events detected this session.</div>
          )}
        </div>

        {/* U-F: Auto-isolate subsection */}
        <div className="border-t border-white/10 pt-3 flex flex-col gap-2">
          <div className="flex flex-wrap items-center gap-3">
            <div className="text-sm font-semibold">Auto-isolate</div>
            <Switch
              checked={autoSandboxRunning}
              disabled={autoSandboxBusy}
              onChange={(e) => toggleAutoSandbox((e.target as HTMLInputElement).checked)}
              label="Enable auto-isolate monitor"
            />
            <Tag
              minimal
              intent={autoSandboxRunning ? (autoSandboxMode === 'enforce' ? 'danger' : 'success') : 'none'}
              className="font-mono"
            >
              {autoSandboxRunning ? autoSandboxMode.toUpperCase() : 'OFF'}
            </Tag>
            {autoActions.length > 0 && (
              <Button
                icon="trash"
                minimal
                small
                onClick={clearAutoActions}
                disabled={autoSandboxBusy}
              >
                Clear
              </Button>
            )}
            {autoSandboxBusy && <Spinner size={14} />}
          </div>

          <div className="text-xs opacity-60">
            Watches for untrusted USB devices on attach. <strong>Observe</strong> (default) alerts
            only — no enforcement. <strong>Enforce</strong> auto-quarantines removable mass-storage
            via the Pro sidecar so its files are inaccessible until you approve it. Requires USB
            monitoring to be enabled first.
          </div>

          {/* Mode selector */}
          {autoSandboxRunning && (
            <div className="flex items-center gap-2 mt-1">
              <div className="text-xs opacity-70">Mode:</div>
              {(['off', 'observe', 'enforce'] as AutoSandboxMode[]).map((m) => (
                <Button
                  key={m}
                  small
                  minimal={autoSandboxMode !== m}
                  intent={
                    autoSandboxMode === m
                      ? m === 'enforce'
                        ? 'danger'
                        : m === 'observe'
                          ? 'warning'
                          : undefined
                      : undefined
                  }
                  disabled={autoSandboxBusy}
                  onClick={() => void setAutoSandboxModeCmd(m)}
                >
                  {m.charAt(0).toUpperCase() + m.slice(1)}
                </Button>
              ))}
            </div>
          )}

          {/* ENFORCE danger note — always shown when mode is enforce */}
          {autoSandboxMode === 'enforce' && autoSandboxRunning && (
            <div
              className="text-xs font-semibold px-2 py-1 border"
              style={{ color: 'var(--color-danger)', borderColor: 'var(--color-danger)' }}
            >
              ENFORCE is active: every untrusted removable drive will be quarantined on attach —
              its files will be inaccessible until you approve it from the USB Intelligence panel.
              This can block input devices if HID mode is also enabled. Use with care.
            </div>
          )}

          {/* Recent auto-action log */}
          {autoActions.length > 0 && (
            <div className="flex flex-col gap-1 mt-1">
              <div className="text-xs font-semibold opacity-70">Recent auto-actions</div>
              {autoActions.map((a, i) => (
                <div
                  key={`${a.deviceKey}-${a.time}-${i}`}
                  className="flex items-start gap-2 border-t border-white/10 pt-1"
                >
                  <Tag
                    minimal
                    intent={
                      a.action === 'quarantine'
                        ? a.enforced
                          ? 'danger'
                          : 'warning'
                        : a.action === 'alert'
                          ? 'warning'
                          : undefined
                    }
                    className="font-mono"
                  >
                    {a.action === 'quarantine'
                      ? a.enforced
                        ? 'QUARANTINED'
                        : 'QUAR-FAILED'
                      : a.action === 'alert'
                        ? 'ALERT'
                        : 'IGNORE'}
                  </Tag>
                  <div className="flex-1">
                    <div className="text-sm font-medium">{a.friendlyName}</div>
                    <div className="font-mono text-xs opacity-60">{a.detail}</div>
                    <div className="font-mono text-xs opacity-50">{a.time}</div>
                  </div>
                </div>
              ))}
            </div>
          )}

          {autoActions.length === 0 && autoSandboxRunning && (
            <div className="text-xs opacity-50">No auto-isolate events this session.</div>
          )}
        </div>
      </div>
    </SectionCard>
  );
}
