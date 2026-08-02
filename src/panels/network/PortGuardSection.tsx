// src/panels/network/PortGuardSection.tsx
//
// KT: Port Watch is the re-homed paid port-monitoring surface; keep the backend
// command names stable until the Pro sidecar contract is renamed.
// One Add form, one merged table. Each entry can be a honeypot
// listener, a firewall block, or both. Two backends sit behind:
//   - Honeypot listener loop (Rust: network_honeypot::*).
//   - Firewall block rules (PowerShell: Block-Protocol / Get-
//     ProtocolBlocks / Unblock-Protocol routed via useBackend).
// The UI hides this split. The user picks an action, the section
// fans out to whichever backend(s) it implies.

import { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import {
  Button,
  Icon,
  InputGroup,
  NumericInput,
  Spinner,
  Switch,
  Tag,
  Tooltip,
} from '@/components/ui/bp';
import SectionCard from '../../components/shared/SectionCard';
import { WebRTCHeaderButton } from '../../components/network/WebRTCLeakCard';
import useBackend from '../../hooks/useBackend';

// ═══════════════════════════════════════════════════════════════════════
// Types + sub-components
// ═══════════════════════════════════════════════════════════════════════

interface HoneypotStatus {
  running: boolean;
  armedPorts: number[];
  conflictingPorts: number[];
  bindAllInterfaces: boolean;
}
interface HoneypotHit {
  port: number;
  service: string;
  peer: string;
  peekHex: string;
  detectedAt: string;
}
export interface PortEntry {
  port: number;
  label: string;
  enabled: boolean;
  custom: boolean;
}
export interface FirewallBlock {
  Name: string;
  Protocol: string;
  Port: string;
  Direction: string;
  Enabled: boolean | number;
}
interface PingBlockStatus {
  blocked: boolean;
}

/** Three independent action flags. The user picks any combination
 *  via visual toggles in the action picker — e.g. "honeypot +
 *  outbound block" is a normal combo, not a one-off enum member. */
interface ActionFlags {
  honeypot: boolean;
  blockInbound: boolean;
  blockOutbound: boolean;
}

const DEFAULT_FLAGS: ActionFlags = {
  honeypot: true,
  blockInbound: false,
  blockOutbound: false,
};

function flagsAnyOn(f: ActionFlags): boolean {
  return f.honeypot || f.blockInbound || f.blockOutbound;
}

/** Honeypot only supports a single TCP port. The form's port field is
 *  a NumericInput so it's always a single number when populated via
 *  the inline form, but presets and the firewall side can carry
 *  ranges ("8000-9000") or lists ("137,138,139"). Detect those so the
 *  picker can disable the honeypot toggle. */
function isSinglePort(port: number | string | ''): boolean {
  if (port === '') return false;
  if (typeof port === 'number') return Number.isFinite(port) && port > 0 && port <= 65535;
  return /^\d+$/.test(port.trim());
}

/** "Pointless" combo: honeypot listener with an inbound firewall
 *  block on the same port. The firewall drops the probe before the
 *  honeypot listener sees it, so the honeypot can never fire. We warn
 *  but still allow submit — there are edge cases (preparing rules to
 *  toggle independently later) where the user might want this. */
function isHoneypotEclipsedByFirewall(f: ActionFlags): boolean {
  return f.honeypot && f.blockInbound;
}

/** Ports the user almost certainly wants outbound traffic on. Blocking
 *  outbound here is usually a mistake (breaks DNS / HTTPS / etc). We
 *  warn before submit. */
const ESSENTIAL_OUTBOUND_PORTS = new Set<number>([
  53, // DNS
  80, // HTTP
  443, // HTTPS
  587, // SMTP submission
  993, // IMAPS
  995, // POP3S
]);

function isEssentialOutbound(port: number | string | '', f: ActionFlags): boolean {
  if (!f.blockOutbound) return false;
  if (typeof port === 'number') return ESSENTIAL_OUTBOUND_PORTS.has(port);
  if (typeof port === 'string' && /^\d+$/.test(port.trim())) {
    return ESSENTIAL_OUTBOUND_PORTS.has(Number(port.trim()));
  }
  return false;
}


/** Firewall preset chips by category. Used by the BulkPresetsPopover
 *  for one-click multi-select prefill of the Add flow. Full catalogue
 *  restored 2026-05 (earlier compaction pass trimmed it). */
const PRESETS: Record<string, { name: string; port: string }[]> = {
  'Remote Access': [
    { name: 'RDP', port: '3389' },
    { name: 'SSH', port: '22' },
    { name: 'Telnet', port: '23' },
    { name: 'VNC', port: '5900' },
    { name: 'TeamViewer', port: '5938' },
  ],
  Web: [
    { name: 'HTTP', port: '80' },
    { name: 'HTTPS', port: '443' },
    { name: 'HTTP-Alt', port: '8080' },
    { name: 'HTTPS-Alt', port: '8443' },
    { name: 'HTTP-Dev', port: '3000' },
    { name: 'HTTP-Dev2', port: '5173' },
  ],
  'File Sharing': [
    { name: 'SMB', port: '445' },
    { name: 'NetBIOS', port: '137,138,139' },
    { name: 'FTP', port: '21' },
    { name: 'FTP-Data', port: '20' },
    { name: 'SFTP', port: '22' },
  ],
  Database: [
    { name: 'MySQL', port: '3306' },
    { name: 'PostgreSQL', port: '5432' },
    { name: 'MSSQL', port: '1433' },
    { name: 'Redis', port: '6379' },
    { name: 'MongoDB', port: '27017' },
    { name: 'Elasticsearch', port: '9200' },
  ],
  Mail: [
    { name: 'SMTP', port: '25' },
    { name: 'SMTP-TLS', port: '587' },
    { name: 'SMTPS', port: '465' },
    { name: 'POP3S', port: '995' },
    { name: 'IMAPS', port: '993' },
  ],
  Gaming: [
    { name: 'Steam', port: '27015,27036' },
    { name: 'Xbox-Live', port: '3074' },
    { name: 'PlayStation', port: '1935,3478,3479' },
    { name: 'Epic-Games', port: '5222' },
    { name: 'Battle.net', port: '1119' },
    { name: 'Minecraft', port: '25565' },
  ],
  'Meetings': [
    { name: 'Zoom', port: '8801,8802' },
    { name: 'Teams', port: '3478,3479,3480' },
  ],
  'Torrent / P2P': [
    { name: 'BitTorrent', port: '6881-6889' },
    { name: 'uTorrent', port: '6969' },
  ],
  'IoT': [
    { name: 'MQTT', port: '1883' },
    { name: 'MQTT-TLS', port: '8883' },
    { name: 'CoAP', port: '5683' },
  ],
};

/** Visual callout replacement (BP Callouts fight our global CSS). */
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
        <div style={{ fontSize: 12, fontWeight: 700, color: palette.color, lineHeight: 1.3 }}>
          {title}
        </div>
        <div style={{ fontSize: 11, lineHeight: 1.45, color: 'var(--color-text-secondary)' }}>
          {children}
        </div>
      </div>
    </div>
  );
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <span
      style={{
        fontSize: 10,
        fontWeight: 700,
        textTransform: 'uppercase',
        letterSpacing: 0.6,
        color: 'var(--color-text-muted)',
      }}
    >
      {children}
    </span>
  );
}

function formatTime(iso: string): string {
  if (!iso) return '—';
  try {
    return new Date(iso).toLocaleString(undefined, {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    });
  } catch {
    return iso;
  }
}

// ═══════════════════════════════════════════════════════════════════════
// Merged row model
// ═══════════════════════════════════════════════════════════════════════

/** A single port-shaped thing in the unified table. Either backend
 *  may have contributed to it; flags say which. */
export interface MergedEntry {
  /** Stable react key. */
  key: string;
  /** Either a numeric port (single-port honeypot/firewall row) or a
   *  raw firewall port spec like "137,138,139" / "6881-6889". */
  portDisplay: string;
  /** Numeric port if it's a single one — used for matching honeypot
   *  rows against firewall rows. Null for multi-port firewall rules. */
  numericPort: number | null;
  label: string;
  /** Honeypot listener attached. */
  honeypot: boolean;
  honeypotEnabled: boolean;
  honeypotCustom: boolean;
  honeypotState: 'armed' | 'pending' | 'conflict' | 'off' | 'idle';
  /** Firewall block attached (may overlap with honeypot row). */
  firewall: boolean;
  firewallName: string | null;
  firewallDirection: 'Inbound' | 'Outbound' | 'Both' | null;
  firewallProtocol: string | null;
}

export function mergeEntries(
  ports: PortEntry[],
  blocks: FirewallBlock[],
  running: boolean,
  armedSet: Set<number>,
  conflictSet: Set<number>,
): MergedEntry[] {
  const byPort = new Map<number, MergedEntry>();

  // Pass 1: honeypot rows.
  for (const p of ports) {
    const state: MergedEntry['honeypotState'] = running
      ? armedSet.has(p.port)
        ? 'armed'
        : conflictSet.has(p.port)
          ? 'conflict'
          : p.enabled
            ? 'pending'
            : 'off'
      : p.enabled
        ? 'idle'
        : 'off';
    byPort.set(p.port, {
      key: `hp:${p.port}`,
      portDisplay: String(p.port),
      numericPort: p.port,
      label: p.label,
      honeypot: true,
      honeypotEnabled: p.enabled,
      honeypotCustom: p.custom,
      honeypotState: state,
      firewall: false,
      firewallName: null,
      firewallDirection: null,
      firewallProtocol: null,
    });
  }

  // Pass 2: firewall rows. Single-port rules merge into the honeypot
  // row when port matches — but only when exactly one firewall rule
  // claims that port. Two separate rules on the same single port
  // (e.g. an Inbound-only block and a later Outbound-only block) must
  // NOT collapse into one merged row — a shared numeric key would let
  // the second overwrite the first's name/direction in the row object,
  // silently hiding it and leaving it unremovable via the UI. Group by
  // port first so collisions are detected before any merge happens.
  const singlePortBlocks = new Map<number, FirewallBlock[]>();
  const multiPortBlocks: FirewallBlock[] = [];
  for (const b of blocks) {
    const port = b.Port.trim();
    const singlePort = /^\d+$/.test(port) ? Number(port) : null;
    if (singlePort !== null) {
      const list = singlePortBlocks.get(singlePort);
      if (list) list.push(b);
      else singlePortBlocks.set(singlePort, [b]);
    } else {
      multiPortBlocks.push(b);
    }
  }

  const standalone: MergedEntry[] = [];
  const pushStandaloneFirewall = (b: FirewallBlock, singlePort: number | null) => {
    standalone.push({
      key: `fw:${b.Name}`,
      portDisplay: b.Port.trim(),
      numericPort: singlePort,
      label: b.Name,
      honeypot: false,
      honeypotEnabled: false,
      honeypotCustom: false,
      honeypotState: 'off',
      firewall: true,
      firewallName: b.Name,
      firewallDirection: (b.Direction as MergedEntry['firewallDirection']) ?? 'Both',
      firewallProtocol: b.Protocol,
    });
  };
  for (const [singlePort, list] of singlePortBlocks) {
    const row = byPort.get(singlePort);
    if (row && list.length === 1) {
      const b = list[0];
      row.firewall = true;
      row.firewallName = b.Name;
      row.firewallDirection = (b.Direction as MergedEntry['firewallDirection']) ?? 'Both';
      row.firewallProtocol = b.Protocol;
    } else {
      // No honeypot row to merge into, or 2+ independent firewall
      // rules share this port — keep each one its own row so none is
      // silently overwritten/orphaned.
      for (const b of list) pushStandaloneFirewall(b, singlePort);
    }
  }
  for (const b of multiPortBlocks) pushStandaloneFirewall(b, null);

  return [...byPort.values(), ...standalone].sort((a, b) => {
    const aNum = a.numericPort ?? 99999;
    const bNum = b.numericPort ?? 99999;
    return aNum - bNum || a.label.localeCompare(b.label);
  });
}

// ═══════════════════════════════════════════════════════════════════════
// Main component
// ═══════════════════════════════════════════════════════════════════════

export default function PortGuardSection(_props: {
  expanded?: boolean;
  onExpandedChange?: (next: boolean) => void;
} = {}) {
  const { blockProtocol, unblockProtocol, getProtocolBlocks } = useBackend();

  const [status, setStatus] = useState<HoneypotStatus | null>(null);
  const [recent, setRecent] = useState<HoneypotHit[]>([]);
  const [ports, setPorts] = useState<PortEntry[]>([]);
  const [blocks, setBlocks] = useState<FirewallBlock[]>([]);
  const [bindAll, setBindAll] = useState(false);
  const [pingBlocked, setPingBlocked] = useState(false);
  const [toggling, setToggling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showMore, setShowMore] = useState(false);

  // Unified Add-form state
  const [newPort, setNewPort] = useState<number | ''>('');
  const [newLabel, setNewLabel] = useState('');
  const [newFlags, setNewFlags] = useState<ActionFlags>(DEFAULT_FLAGS);
  const [submitting, setSubmitting] = useState(false);

  // Bulk selection of rows in the managed-ports table
  const [selectedKeys, setSelectedKeys] = useState<Set<string>>(new Set());
  const [bulkRemoving, setBulkRemoving] = useState(false);

  // Derived view — declared up here so callbacks below can close over
  // it without hitting TDZ.
  const running = !!status?.running;
  const armed = useMemo(() => status?.armedPorts ?? [], [status?.armedPorts]);
  const armedSet = useMemo(() => new Set(armed), [armed]);
  const conflictSet = useMemo(
    () => new Set(status?.conflictingPorts ?? []),
    [status?.conflictingPorts],
  );
  const merged = useMemo(
    () => mergeEntries(ports, blocks, running, armedSet, conflictSet),
    [ports, blocks, running, armedSet, conflictSet],
  );

  // ── Fetch + subscribe ────────────────────────────────────────────
  const refresh = useCallback(async () => {
    try {
      const [s, r, b, p, ping, fwResp] = await Promise.all([
        invoke<HoneypotStatus>('network_honeypot_status'),
        invoke<HoneypotHit[]>('get_network_honeypot_recent'),
        invoke<boolean>('get_network_honeypot_bind_all_interfaces'),
        invoke<PortEntry[]>('get_network_honeypot_ports'),
        invoke<PingBlockStatus>('get_ping_block_status'),
        getProtocolBlocks(),
      ]);
      setStatus(s);
      setRecent([...r].reverse());
      setBindAll(b);
      setPorts([...p].sort((a, b) => a.port - b.port));
      setPingBlocked(ping.blocked);
      if (fwResp.success && fwResp.data) {
        const raw = (fwResp.data as { blocks?: FirewallBlock | FirewallBlock[] }).blocks;
        setBlocks(Array.isArray(raw) ? raw : raw ? [raw] : []);
      } else {
        setBlocks([]);
      }
    } catch (err) {
      setError(String(err));
    }
  }, [getProtocolBlocks]);

  useEffect(() => {
    void refresh();
    let unlisten: UnlistenFn | null = null;
    (async () => {
      unlisten = await listen<HoneypotHit>('network-honeypot-detected', (e) => {
        setRecent((prev) => [e.payload, ...prev].slice(0, 30));
      });
    })();
    return () => {
      if (unlisten) unlisten();
    };
  }, [refresh]);

  // ── Helpers ──────────────────────────────────────────────────────
  const restartIfRunning = useCallback(async () => {
    if (!status?.running) return;
    await invoke('stop_network_honeypot');
    const s = await invoke<HoneypotStatus>('start_network_honeypot');
    setStatus(s);
  }, [status?.running]);

  const friendlyPaidError = (err: unknown): string => {
    const msg = String(err);
    return msg.includes('PAID:') || msg.toLowerCase().includes('paid')
      ? 'WinCommander Pro required.'
      : msg;
  };

  // ── Actions ──────────────────────────────────────────────────────
  const handleMasterToggle = useCallback(
    async (next: boolean) => {
      setToggling(true);
      setError(null);
      try {
        if (next) {
          const s = await invoke<HoneypotStatus>('start_network_honeypot');
          setStatus(s);
        } else {
          await invoke('stop_network_honeypot');
          await refresh();
        }
      } catch (err) {
        setError(friendlyPaidError(err));
      } finally {
        setToggling(false);
      }
    },
    [refresh],
  );

  const handleBindAll = useCallback(
    async (next: boolean) => {
      setError(null);
      try {
        await invoke('set_network_honeypot_bind_all_interfaces', { value: next });
        setBindAll(next);
        await restartIfRunning();
      } catch (err) {
        setError(friendlyPaidError(err));
      }
    },
    [restartIfRunning],
  );

  const handlePingBlock = useCallback(async (next: boolean) => {
    setError(null);
    try {
      const s = await invoke<PingBlockStatus>('set_ping_block', { enabled: next });
      setPingBlocked(s.blocked);
    } catch (err) {
      setError(friendlyPaidError(err));
    }
  }, []);

  const handleHoneypotEnable = useCallback(
    async (port: number, enabled: boolean) => {
      setError(null);
      setPorts((prev) => prev.map((p) => (p.port === port ? { ...p, enabled } : p)));
      try {
        await invoke('set_network_honeypot_port_enabled', { port, enabled });
        await restartIfRunning();
      } catch (err) {
        setError(friendlyPaidError(err));
        await refresh();
      }
    },
    [refresh, restartIfRunning],
  );

  /** Add one preset (or any port+label pair) using the supplied flags.
   *  Used by both the chip-click picker and the manual Add form. */
  const addOne = useCallback(
    async (item: { name: string; port: string }, flags: ActionFlags) => {
      if (!flagsAnyOn(flags)) {
        throw new Error('Pick at least one action.');
      }
      const portNum = /^\d+$/.test(item.port) ? Number(item.port) : null;
      if (flags.honeypot) {
        if (portNum === null) {
          throw new Error(
            'Port Watch needs a single port — use a firewall block for multi-port specs.',
          );
        }
        await invoke('add_network_honeypot_custom_port', {
          port: portNum,
          label: item.name,
        });
      }
      // Firewall: separate Inbound and Outbound rules so the user can
      // toggle them independently later. Direction "Both" is only used
      // when both are checked.
      if (flags.blockInbound && flags.blockOutbound) {
        const result = await blockProtocol(item.name, item.port, 'TCP', 'Both');
        if (!result.success) {
          throw new Error(result.error || 'Failed to add firewall rule');
        }
      } else if (flags.blockInbound) {
        const result = await blockProtocol(item.name, item.port, 'TCP', 'Inbound');
        if (!result.success) {
          throw new Error(result.error || 'Failed to add firewall rule');
        }
      } else if (flags.blockOutbound) {
        const result = await blockProtocol(item.name, item.port, 'TCP', 'Outbound');
        if (!result.success) {
          throw new Error(result.error || 'Failed to add firewall rule');
        }
      }
    },
    [blockProtocol],
  );

  /** Submit Add: fans out to honeypot and/or firewall backends. */
  const handleAdd = useCallback(async () => {
    if (newPort === '' || !newLabel.trim()) {
      setError('Port number and label are both required.');
      return;
    }
    if (!flagsAnyOn(newFlags)) {
      setError('Pick at least one action — Watch, Block In, or Block Out.');
      return;
    }
    setSubmitting(true);
    setError(null);
    try {
      await addOne({ name: newLabel.trim(), port: String(newPort) }, newFlags);
      setNewPort('');
      setNewLabel('');
      await refresh();
      if (newFlags.honeypot) await restartIfRunning();
    } catch (err) {
      setError(friendlyPaidError(err));
    } finally {
      setSubmitting(false);
    }
  }, [newPort, newLabel, newFlags, addOne, refresh, restartIfRunning]);

  /** Remove from whichever backend(s) owned this row. */
  const removeOne = useCallback(
    async (row: MergedEntry) => {
      if (row.honeypot && row.honeypotCustom && row.numericPort !== null) {
        await invoke('remove_network_honeypot_custom_port', { port: row.numericPort });
      }
      if (row.firewall && row.firewallName) {
        await unblockProtocol(row.firewallName);
      }
    },
    [unblockProtocol],
  );

  const handleRemoveRow = useCallback(
    async (row: MergedEntry) => {
      setError(null);
      try {
        await removeOne(row);
        setSelectedKeys((prev) => {
          if (!prev.has(row.key)) return prev;
          const next = new Set(prev);
          next.delete(row.key);
          return next;
        });
        await refresh();
        await restartIfRunning();
      } catch (err) {
        setError(friendlyPaidError(err));
      }
    },
    [removeOne, refresh, restartIfRunning],
  );

  /** Click-to-prefill from the chip grid. Drops the chip's port +
   *  label into the manual Add row; the user picks the action
   *  toggles + hits Add. */
  const handleChipPrefill = useCallback(
    (item: { name: string; port: string }) => {
      setError(null);
      setNewPort(/^\d+$/.test(item.port) ? Number(item.port) : '');
      setNewLabel(item.name);
    },
    [],
  );

  /** Bulk-remove every currently-selected removable row. */
  const handleBulkRemove = useCallback(async () => {
    if (selectedKeys.size === 0) return;
    setError(null);
    setBulkRemoving(true);
    const failures: string[] = [];
    try {
      // Snapshot rows by key — `merged` recomputes between awaits and
      // we don't want to lose the reference.
      const targets = merged.filter((r) => selectedKeys.has(r.key));
      for (const row of targets) {
        try {
          await removeOne(row);
        } catch (err) {
          failures.push(`${row.label}: ${String(err)}`);
        }
      }
      setSelectedKeys(new Set());
      await refresh();
      await restartIfRunning();
      if (failures.length > 0) {
        setError(
          `${targets.length - failures.length}/${targets.length} removed. Failures: ${failures.join('; ')}`,
        );
      }
    } finally {
      setBulkRemoving(false);
    }
  }, [selectedKeys, merged, removeOne, refresh, restartIfRunning]);

  const toggleRowSelection = useCallback((key: string, on: boolean) => {
    setSelectedKeys((prev) => {
      const next = new Set(prev);
      if (on) next.add(key);
      else next.delete(key);
      return next;
    });
  }, []);

  const setSelectionForAll = useCallback((rows: MergedEntry[], on: boolean) => {
    setSelectedKeys((prev) => {
      const next = new Set(prev);
      for (const r of rows) {
        if (!r.honeypotCustom && !r.firewall) continue; // can't select non-removable
        if (on) next.add(r.key);
        else next.delete(r.key);
      }
      return next;
    });
  }, []);

  // ── Render helpers ───────────────────────────────────────────────
  const headerRight = (
    <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
      <WebRTCHeaderButton />
      <span style={{ width: 1, height: 12, background: 'var(--color-border)', flexShrink: 0 }} />
      {running ? (
        <Tag intent="success" minimal style={{ fontSize: 9 }}>
          ACTIVE · {armed.length}
        </Tag>
      ) : (
        <Tag minimal style={{ fontSize: 9, opacity: 0.6 }}>
          INACTIVE
        </Tag>
      )}
    </div>
  );

  return (
    <SectionCard
      title="Port Watch &amp; Firewall"
      icon="shield"
      headerRight={headerRight}
      className="ncr-stretch-card"
    >
      <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
        {error && (
          <IntelNotice intent="danger" icon="error" title="Couldn't apply">
            {error}
          </IntelNotice>
        )}

        {/* ── Compact toolbar: master + LAN + ping ─────────────── */}
        <Toolbar
          running={running}
          toggling={toggling}
          onMasterToggle={handleMasterToggle}
          bindAll={bindAll}
          onBindAllChange={handleBindAll}
          pingBlocked={pingBlocked}
          onPingBlockChange={handlePingBlock}
        />

        {/* ── Manual Add row (port + label + visual action toggles) */}
        <AddRow
          port={newPort}
          label={newLabel}
          flags={newFlags}
          onPortChange={setNewPort}
          onLabelChange={setNewLabel}
          onFlagsChange={setNewFlags}
          onSubmit={handleAdd}
          submitting={submitting}
        />

        {/* ── Common presets stay visible; Show more expands the full catalogue. */}
        <ChipGrid onPickPreset={handleChipPrefill} showAll={showMore} />

        {/* ── Managed ports table with bulk-select ─────────────── */}
        <ManagedPortsTable
          rows={merged}
          selectedKeys={selectedKeys}
          onToggleSelect={toggleRowSelection}
          onToggleSelectAll={(on) => setSelectionForAll(merged, on)}
          onBulkRemove={handleBulkRemove}
          bulkRemoving={bulkRemoving}
          onHoneypotEnable={handleHoneypotEnable}
          onRemove={handleRemoveRow}
        />

        <Button
          minimal
          small
          icon={showMore ? 'chevron-up' : 'chevron-down'}
          onClick={() => setShowMore((next) => !next)}
          style={{ alignSelf: 'flex-start', fontFamily: 'var(--font-mono)', fontSize: 10 }}
        >
          {showMore ? 'Hide port presets' : 'Show port presets'}
        </Button>

        {showMore && (
          <>
            {/* ── Recent probes ───────────────────────────────────── */}
            <RecentProbes recent={recent} running={running} onClear={() => setRecent([])} />
          </>
        )}
      </div>
    </SectionCard>
  );
}

// ═══════════════════════════════════════════════════════════════════════
// Sub-components
// ═══════════════════════════════════════════════════════════════════════

/** Single horizontal toolbar — master arm + LAN-expose + block-pings.
 *  Compact pill icons; tooltips carry the long-form copy so we don't
 *  burn vertical space on always-visible descriptions. */
function Toolbar({
  running,
  toggling,
  onMasterToggle,
  bindAll,
  onBindAllChange,
  pingBlocked,
  onPingBlockChange,
}: {
  running: boolean;
  toggling: boolean;
  onMasterToggle: (v: boolean) => void;
  bindAll: boolean;
  onBindAllChange: (v: boolean) => void;
  pingBlocked: boolean;
  onPingBlockChange: (v: boolean) => void;
}) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 10,
        padding: '6px 10px',
        background: 'var(--color-bg-secondary)',
        border: '1px solid var(--color-border)',
        borderRadius: 4,
        flexWrap: 'wrap',
      }}
    >
      <Switch
        checked={running}
        onChange={(e) => onMasterToggle((e.target as HTMLInputElement).checked)}
        disabled={toggling}
        label={running ? 'Armed' : 'Arm honeypot'}
        style={{ marginBottom: 0 }}
      />
      {toggling && <Spinner size={14} />}
      <span style={{ width: 1, height: 18, background: 'var(--color-border)' }} />
      <Tooltip
        content={
          bindAll
            ? 'Listening on 0.0.0.0 — LAN devices can hit the honeypot. Click to switch back to loopback.'
            : 'Loopback only (default). Click to expose honeypot ports on the LAN — only do this on a network you control.'
        }
        placement="top"
      >
        <span style={{ display: 'inline-flex', alignItems: 'center', gap: 4 }}>
          <Switch
            checked={bindAll}
            onChange={(e) => onBindAllChange((e.target as HTMLInputElement).checked)}
            label="Expose to LAN"
            style={{ marginBottom: 0 }}
          />
          {bindAll && <Icon icon="warning-sign" size={11} color="var(--color-warning)" />}
        </span>
      </Tooltip>
      <Tooltip
        content={
          pingBlocked
            ? 'ICMPv4 + ICMPv6 echo dropped — host no longer answers `ping`. Click to allow pings again.'
            : 'Allowing pings (Windows default). Click to drop inbound ICMP echo. Requires admin.'
        }
        placement="top"
      >
        <Switch
          checked={pingBlocked}
          onChange={(e) => onPingBlockChange((e.target as HTMLInputElement).checked)}
          label="Block pings"
          style={{ marginBottom: 0 }}
        />
      </Tooltip>
    </div>
  );
}

/** Three visual toggle pills: Watch / Block In / Block Out.
 *  Each is an icon + label pill that flips on/off independently;
 *  active state shows a colored background + border so the pick is
 *  obvious without reading text. When `port` is multi-spec
 *  ("8000-9000", "137,138,139") the watch pill is disabled. */
function ActionTogglePicker({
  port,
  flags,
  onChange,
  compact = false,
}: {
  port?: number | string | '';
  flags: ActionFlags;
  onChange: (next: ActionFlags) => void;
  compact?: boolean;
}) {
  const pad = compact ? '3px 8px' : '5px 10px';
  const gap = compact ? 6 : 8;
  const fs = compact ? 11 : 12;
  const portOk = port === undefined ? true : isSinglePort(port);

  type Pill = {
    key: keyof ActionFlags;
    icon: React.ComponentProps<typeof Icon>['icon'];
    label: string;
    color: string;
    tooltip: string;
    disabled?: boolean;
  };
  const pills: Pill[] = [
    {
      key: 'honeypot',
      icon: 'eye-open',
      label: 'Watch',
      color: 'var(--color-success)',
      tooltip: portOk
        ? 'Watch this port for unexpected probes.'
        : 'Port Watch needs a single TCP port — ranges and lists aren\'t supported.',
      disabled: !portOk,
    },
    {
      key: 'blockInbound',
      icon: 'arrow-left',
      label: 'Block In',
      color: 'var(--color-danger)',
      tooltip: 'Drop traffic arriving on this port (inbound firewall rule).',
    },
    {
      key: 'blockOutbound',
      icon: 'arrow-right',
      label: 'Block Out',
      color: 'var(--color-danger)',
      tooltip: 'Drop traffic leaving on this port (outbound firewall rule).',
    },
  ];

  return (
    <div style={{ display: 'flex', gap, flexWrap: 'wrap' }}>
      {pills.map((p) => {
        const on = flags[p.key] && !p.disabled;
        const dim = p.disabled;
        return (
          <Tooltip key={p.key} content={p.tooltip} placement="top">
            <button
              type="button"
              disabled={p.disabled}
              onClick={() => {
                if (p.disabled) return;
                onChange({ ...flags, [p.key]: !on });
              }}
              style={{
                display: 'inline-flex',
                alignItems: 'center',
                gap: 6,
                padding: pad,
                borderRadius: 4,
                border: `1px solid ${on ? p.color : 'var(--color-border)'}`,
                background: on
                  ? `color-mix(in srgb, ${p.color} 18%, transparent)`
                  : 'transparent',
                color: dim
                  ? 'var(--color-text-muted)'
                  : on
                    ? p.color
                    : 'var(--color-text-secondary)',
                fontSize: fs,
                fontWeight: on ? 700 : 500,
                cursor: dim ? 'not-allowed' : 'pointer',
                opacity: dim ? 0.5 : 1,
                transition: 'all 100ms ease',
                userSelect: 'none',
              }}
            >
              <Icon icon={p.icon} size={fs} color={on ? p.color : 'var(--color-text-muted)'} />
              {p.label}
            </button>
          </Tooltip>
        );
      })}
    </div>
  );
}

/** Edge-case warnings shown under the picker. Render nothing when
 *  the combo is clean. */
function CombinationWarnings({
  port,
  flags,
}: {
  port: number | string | '';
  flags: ActionFlags;
}) {
  const eclipsed = isHoneypotEclipsedByFirewall(flags);
  const essentialOut = isEssentialOutbound(port, flags);
  if (!eclipsed && !essentialOut) return null;
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 6, marginTop: 6 }}>
      {eclipsed && (
        <IntelNotice
          intent="warning"
          icon="info-sign"
          title="Port Watch will not fire with Block In on the same port"
        >
          The inbound firewall rule drops the probe before the watcher
          sees it. Use just Block In, or drop Block In to keep watch
          visibility.
        </IntelNotice>
      )}
      {essentialOut && (
        <IntelNotice
          intent="warning"
          icon="warning-sign"
          title="Blocking outbound on an essential port"
        >
          This port is typically required for normal operation (DNS,
          HTTPS, mail). Blocking outbound may break apps on this host.
        </IntelNotice>
      )}
    </div>
  );
}

/** Always-visible chip grid. Click a chip → prefill the Add form
 *  below with that port + label. The user then picks the action
 *  toggles inline and confirms. No popover, no hover surprise. */
function ChipGrid({
  onPickPreset,
  showAll,
}: {
  onPickPreset: (item: { name: string; port: string }) => void;
  showAll: boolean;
}) {
  const entries: Array<[string, { name: string; port: string }[]]> = showAll
    ? Object.entries(PRESETS)
    : [
        ['Common', [
          { name: 'RDP', port: '3389' },
          { name: 'SSH', port: '22' },
          { name: 'HTTP', port: '80' },
          { name: 'HTTPS', port: '443' },
          { name: 'SMB', port: '445' },
          { name: 'MySQL', port: '3306' },
        ]],
      ];

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: 6,
        padding: 10,
        background: 'var(--color-bg-secondary)',
        border: '1px solid var(--color-border)',
        borderRadius: 4,
      }}
    >
      <span
        style={{
          fontSize: 10,
          fontWeight: 700,
          textTransform: 'uppercase',
          letterSpacing: 0.6,
          color: 'var(--color-text-muted)',
          marginBottom: 2,
        }}
      >
        {showAll ? 'All presets · click a port to prefill the form' : 'Common ports · click to prefill'}
      </span>
      {entries.map(([category, items]) => (
        <div
          key={category}
          style={{ display: 'flex', flexDirection: 'column', alignItems: 'stretch', gap: 6 }}
        >
          <span
            style={{
              fontFamily: 'var(--font-mono)',
              fontSize: 9,
              color: 'var(--color-text-muted)',
              textTransform: 'uppercase',
              paddingLeft: 2,
            }}
          >
            {category}
          </span>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
            {items.map((p) => (
              <Tag
                key={`${p.name}-${p.port}`}
                interactive
                minimal
                onClick={() => onPickPreset(p)}
                style={{ fontSize: 11, cursor: 'pointer' }}
              >
                {p.name} <code style={{ opacity: 0.55, fontSize: 9 }}>{p.port}</code>
              </Tag>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

/** Inline Add form for custom (non-preset) ports. Three visual
 *  action toggles replace the old text dropdown; same data model as
 *  the chip-prefill flow. Edge-case warnings render under the form. */
function AddRow({
  port,
  label,
  flags,
  onPortChange,
  onLabelChange,
  onFlagsChange,
  onSubmit,
  submitting,
}: {
  port: number | '';
  label: string;
  flags: ActionFlags;
  onPortChange: (v: number | '') => void;
  onLabelChange: (v: string) => void;
  onFlagsChange: (v: ActionFlags) => void;
  onSubmit: () => void;
  submitting: boolean;
}) {
  // If port becomes invalid for honeypot, clear that flag so a stale
  // pick doesn't sit hidden behind a disabled pill.
  const honeypotOk = isSinglePort(port);
  const effectiveFlags: ActionFlags = honeypotOk
    ? flags
    : { ...flags, honeypot: false };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 0 }}>
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
        <NumericInput
          placeholder="Port"
          value={port}
          onValueChange={(v) => onPortChange(Number.isFinite(v) ? v : '')}
          min={1}
          max={65535}
          buttonPosition="none"
          small
          style={{ width: 80 }}
        />
        <InputGroup
          placeholder="Label / rule name"
          value={label}
          onChange={(e) => onLabelChange((e.target as HTMLInputElement).value)}
          maxLength={24}
          small
          style={{ flex: 1, minWidth: 140 }}
        />
        <ActionTogglePicker
          port={port}
          flags={effectiveFlags}
          onChange={onFlagsChange}
          compact
        />
        <Button
          icon="add"
          intent="primary"
          onClick={onSubmit}
          loading={submitting}
          disabled={port === '' || !label.trim() || !flagsAnyOn(effectiveFlags)}
          small
        >
          Add
        </Button>
      </div>
      <CombinationWarnings port={port} flags={effectiveFlags} />
    </div>
  );
}

const GRID_COLS = '28px 64px 1fr 140px 140px 28px';

const COMPACT_PORT_ROWS = 5;

function ManagedPortsTable({
  rows,
  selectedKeys,
  onToggleSelect,
  onToggleSelectAll,
  onBulkRemove,
  bulkRemoving,
  onHoneypotEnable,
  onRemove,
}: {
  rows: MergedEntry[];
  selectedKeys: Set<string>;
  onToggleSelect: (key: string, on: boolean) => void;
  onToggleSelectAll: (on: boolean) => void;
  onBulkRemove: () => void;
  bulkRemoving: boolean;
  onHoneypotEnable: (port: number, enabled: boolean) => void;
  onRemove: (row: MergedEntry) => void;
}) {
  const [showAll, setShowAll] = useState(false);
  const visibleRows = showAll ? rows : rows.slice(0, COMPACT_PORT_ROWS);
  const hiddenCount = rows.length - COMPACT_PORT_ROWS;
  const removableRows = useMemo(
    () => rows.filter((r) => r.honeypotCustom || r.firewall),
    [rows],
  );
  const allSelected =
    removableRows.length > 0 && removableRows.every((r) => selectedKeys.has(r.key));
  const someSelected = removableRows.some((r) => selectedKeys.has(r.key));

  return (
    <div
      style={{
        border: '1px solid var(--color-border)',
        borderRadius: 4,
        overflow: 'hidden',
      }}
    >
      {/* Header — clicking the checkbox toggles select-all on
          removable rows. When >0 are selected, the column titles
          collapse and a bulk-remove action appears in their place. */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: GRID_COLS,
          padding: '4px 10px',
          background: 'var(--color-bg-tertiary)',
          fontSize: 9,
          fontWeight: 700,
          textTransform: 'uppercase',
          letterSpacing: 0.6,
          color: 'var(--color-text-muted)',
          borderBottom: '1px solid var(--color-border)',
          alignItems: 'center',
          gap: 8,
        }}
      >
        <Tooltip
          content={
            allSelected
              ? 'Clear selection'
              : someSelected
                ? 'Select all removable rows'
                : 'Select all removable rows'
          }
          placement="top"
        >
          <span
            onClick={() => onToggleSelectAll(!allSelected)}
            style={{
              cursor: removableRows.length === 0 ? 'not-allowed' : 'pointer',
              opacity: removableRows.length === 0 ? 0.3 : 1,
              display: 'inline-flex',
              alignItems: 'center',
              justifyContent: 'center',
            }}
          >
            <Icon
              icon={allSelected ? 'tick' : someSelected ? 'minus' : 'blank'}
              size={13}
              color={
                allSelected || someSelected
                  ? 'var(--color-accent)'
                  : 'var(--color-text-muted)'
              }
            />
          </span>
        </Tooltip>
        {selectedKeys.size > 0 ? (
          <span
            style={{
              gridColumn: '2 / span 5',
              display: 'flex',
              alignItems: 'center',
              gap: 8,
              color: 'var(--color-text-primary)',
              fontWeight: 700,
              textTransform: 'none',
              letterSpacing: 0,
              fontSize: 11,
            }}
          >
            <span>{selectedKeys.size} selected</span>
            <span style={{ flex: 1 }} />
            <Button
              small
              minimal
              onClick={() => onToggleSelectAll(false)}
            >
              Clear
            </Button>
            <Button
              small
              intent="danger"
              icon="trash"
              onClick={onBulkRemove}
              loading={bulkRemoving}
            >
              Remove {selectedKeys.size}
            </Button>
          </span>
        ) : (
          <>
            <span>Port</span>
            <span>Label / Rule</span>
            <span>Watch</span>
            <span>Firewall</span>
            <span style={{ textAlign: 'right' }}>{rows.length}</span>
          </>
        )}
      </div>
      {rows.length === 0 ? (
        <div
          style={{
            padding: '14px 10px',
            textAlign: 'center',
            fontSize: 11,
            color: 'var(--color-text-muted)',
            fontStyle: 'italic',
          }}
        >
          Nothing configured. Use the Add row above (or the Presets popover
          for bulk-add) to declare ports.
        </div>
      ) : (
        <>
          <div>
            {visibleRows.map((row) => (
              <ManagedPortRow
                key={row.key}
                row={row}
                selected={selectedKeys.has(row.key)}
                onToggleSelect={onToggleSelect}
                onHoneypotEnable={onHoneypotEnable}
                onRemove={onRemove}
              />
            ))}
          </div>
          {!showAll && hiddenCount > 0 && (
            <button
              onClick={() => setShowAll(true)}
              style={{
                width: '100%',
                padding: '6px 10px',
                background: 'var(--color-bg-tertiary)',
                border: 'none',
                borderTop: '1px solid var(--color-border)',
                color: 'var(--color-text-muted)',
                fontSize: 10,
                fontFamily: 'var(--font-mono)',
                fontWeight: 700,
                letterSpacing: 0.5,
                cursor: 'pointer',
                textAlign: 'center',
              }}
            >
              Show {hiddenCount} more
            </button>
          )}
          {showAll && rows.length > COMPACT_PORT_ROWS && (
            <button
              onClick={() => setShowAll(false)}
              style={{
                width: '100%',
                padding: '6px 10px',
                background: 'var(--color-bg-tertiary)',
                border: 'none',
                borderTop: '1px solid var(--color-border)',
                color: 'var(--color-text-muted)',
                fontSize: 10,
                fontFamily: 'var(--font-mono)',
                fontWeight: 700,
                letterSpacing: 0.5,
                cursor: 'pointer',
                textAlign: 'center',
              }}
            >
              Show less
            </button>
          )}
        </>
      )}
    </div>
  );
}

function ManagedPortRow({
  row,
  selected,
  onToggleSelect,
  onHoneypotEnable,
  onRemove,
}: {
  row: MergedEntry;
  selected: boolean;
  onToggleSelect: (key: string, on: boolean) => void;
  onHoneypotEnable: (port: number, enabled: boolean) => void;
  onRemove: (row: MergedEntry) => void;
}) {
  const stateTag =
    row.honeypotState === 'armed'
      ? { color: 'var(--color-success)', text: 'ACTIVE' }
      : row.honeypotState === 'conflict'
        ? { color: 'var(--color-warning)', text: 'IN-USE' }
        : row.honeypotState === 'pending'
          ? { color: 'var(--color-info)', text: 'PENDING' }
          : row.honeypotState === 'idle'
            ? { color: 'var(--color-text-muted)', text: 'IDLE' }
            : null;

  return (
    <div
      style={{
        display: 'grid',
        gridTemplateColumns: GRID_COLS,
        alignItems: 'center',
        padding: '4px 10px',
        borderBottom: '1px solid var(--color-border)',
        background: selected
          ? 'color-mix(in srgb, var(--color-accent) 8%, var(--color-bg-secondary))'
          : 'var(--color-bg-secondary)',
        fontSize: 11,
        gap: 8,
        minHeight: 32,
      }}
    >
      {row.honeypotCustom || row.firewall ? (
        <Tooltip content={selected ? 'Deselect row' : 'Select for bulk remove'} placement="top">
          <span
            onClick={() => onToggleSelect(row.key, !selected)}
            style={{
              cursor: 'pointer',
              display: 'inline-flex',
              alignItems: 'center',
              justifyContent: 'center',
            }}
          >
            <Icon
              icon={selected ? 'tick' : 'blank'}
              size={13}
              color={selected ? 'var(--color-accent)' : 'var(--color-text-muted)'}
            />
          </span>
        </Tooltip>
      ) : (
        <Tooltip
          content="Built-in catalogue port — can only be toggled, not removed"
          placement="top"
        >
          <Icon icon="lock" size={11} color="var(--color-text-muted)" />
        </Tooltip>
      )}
      <code
        style={{
          fontFamily: 'var(--font-mono)',
          fontSize: 11,
          fontWeight: 700,
          color: 'var(--color-text-primary)',
        }}
      >
        {row.portDisplay}
      </code>
      <span style={{ fontWeight: 600, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
        {row.label}
      </span>

      {/* ── Watch column ─────────────────────────────────────── */}
      <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
        {row.honeypot && row.numericPort !== null ? (
          <>
            <Tooltip
              content={
                row.honeypotEnabled
                  ? 'Port Watch enabled — toggle off to skip on next arm'
                  : 'Port Watch disabled — toggle on to include on next arm'
              }
              placement="top"
            >
              <Switch
                checked={row.honeypotEnabled}
                aria-label={`${row.honeypotEnabled ? 'Disable' : 'Enable'} Port Watch for ${row.label} on ${row.portDisplay}`}
                onChange={(e) =>
                  onHoneypotEnable(row.numericPort!, (e.target as HTMLInputElement).checked)
                }
                style={{ marginBottom: 0 }}
              />
            </Tooltip>
            {stateTag && (
              <Tooltip
                content={
                  stateTag.text === 'ACTIVE'
                    ? 'Bound and watching for probes'
                    : stateTag.text === 'IN-USE'
                      ? 'A real service owns this port — watcher skipped it'
                      : stateTag.text === 'PENDING'
                        ? 'Will arm on next restart of the watch loop'
                        : 'Loop inactive — port is configured but not listening'
                }
                placement="top"
              >
                <span
                  style={{
                    fontSize: 9,
                    fontWeight: 800,
                    letterSpacing: 0.4,
                    padding: '1px 5px',
                    borderRadius: 3,
                    color: stateTag.color,
                    border: `1px solid ${stateTag.color}`,
                  }}
                >
                  {stateTag.text}
                </span>
              </Tooltip>
            )}
          </>
        ) : (
          <span style={{ fontSize: 11, color: 'var(--color-text-muted)' }}>—</span>
        )}
      </div>

      {/* ── Firewall column ──────────────────────────────────── */}
      <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
        {row.firewall ? (
          <Tooltip
            content={`Firewall block · ${row.firewallProtocol} · ${row.firewallDirection}`}
            placement="top"
          >
            <span
              style={{
                fontSize: 9,
                fontWeight: 800,
                letterSpacing: 0.4,
                padding: '2px 6px',
                borderRadius: 3,
                color: 'var(--color-danger)',
                border: '1px solid var(--color-danger)',
                background: 'color-mix(in srgb, var(--color-danger) 10%, transparent)',
              }}
            >
              BLOCK ·{' '}
              {row.firewallDirection === 'Inbound'
                ? 'IN'
                : row.firewallDirection === 'Outbound'
                  ? 'OUT'
                  : 'BOTH'}{' '}
              · {row.firewallProtocol}
            </span>
          </Tooltip>
        ) : (
          <span style={{ fontSize: 11, color: 'var(--color-text-muted)' }}>—</span>
        )}
      </div>

      {/* ── Remove ──────────────────────────────────────────── */}
      {row.honeypotCustom || row.firewall ? (
        <Tooltip content="Remove this entry" placement="top">
          <Button
            icon="cross"
            minimal
            intent="danger"
            small
            onClick={() => onRemove(row)}
            aria-label={`Remove ${row.label} on ${row.portDisplay}`}
            style={{ minHeight: 22, minWidth: 22, padding: 0 }}
          />
        </Tooltip>
      ) : (
        <span />
      )}
    </div>
  );
}

function RecentProbes({
  recent,
  running,
  onClear,
}: {
  recent: HoneypotHit[];
  running: boolean;
  onClear: () => void;
}) {
  return (
    <div>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          marginBottom: 6,
        }}
      >
        <SectionLabel>Recent honeypot probes ({recent.length})</SectionLabel>
        {recent.length > 0 && (
          <Button icon="trash" minimal small onClick={onClear}>
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
            ? 'Nothing has probed a honeypot yet. Try `Test-NetConnection localhost -Port 22`.'
            : 'Arm honeypot listeners to start watching.'}
        </div>
      ) : (
        <div
          style={{
            display: 'flex',
            flexDirection: 'column',
            gap: 4,
            maxHeight: 240,
            overflowY: 'auto',
          }}
        >
          {recent.map((h, i) => (
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
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                <Tag intent="danger" minimal style={{ fontSize: 10, fontWeight: 700 }}>
                  {h.service}
                </Tag>
                <code style={{ fontSize: 10, color: 'var(--color-text-muted)' }}>tcp/{h.port}</code>
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
              <div style={{ marginTop: 3, fontSize: 10, color: 'var(--color-text-muted)' }}>
                from <code>{h.peer}</code>
                {h.peekHex && (
                  <>
                    {' · '}
                    <code title="First 64 bytes the scanner sent (hex)">
                      peek: {h.peekHex.slice(0, 24)}
                      {h.peekHex.length > 24 ? '…' : ''}
                    </code>
                  </>
                )}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
