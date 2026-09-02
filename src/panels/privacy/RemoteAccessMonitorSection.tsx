// src/panels/privacy/RemoteAccessMonitorSection.tsx
//
// Remote Access Monitor — Privacy monitoring section card.
//
// Detects an INCOMING remote-control session (AnyDesk / TeamViewer /
// RustDesk / VNC / RDP / Chrome RDP / Quick Assist …) and fires a loud
// alert "like the clipboard monitor". Same progressive-disclosure
// pattern as the file/ransomware monitor cards: default view = a
// single ON/OFF + status pill. Expanding "Configure" reveals:
//   - Tool catalogue — per-tool enable switches (trim the watch-list).
//   - Recent detections (last 30, in-memory Pro ring). `info` rows
//     render quiet (tool merely running); `high` rows render danger
//     (red border, tool + peer + relative time). Clear button.
//
// The runtime authority (poll task, catalogue, recent ring) lives in the
// Pro module commander-pro/src/remote_access.rs. The persisted
// source-of-truth for "enabled" + per-tool overrides lives in
// privacy.remoteAccessMonitor; the global useRemoteAccessMonitor hook
// reconciles the runtime into Pro on every settings change. This section
// invokes the Pro-backed commands directly (the NetworkHoneypotSection
// convention).

import { Switch, Icon, Button, Tag } from "@/components/ui/bp";
import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import { showError, showSuccess } from "../../utils/toast";
import SectionCard from "../../components/shared/SectionCard";
import { useAppConfirm } from "../../components/shared/AppConfirmDialog";
import { useAppState } from "../../context/AppContext";
import { applyMachineSetting } from "../../lib/machineSettingsClient";
import PrivacyEventTable from './PrivacyEventTable';

interface ToolEntry {
  id: string;
  label: string;
  processNames: string[];
  ports: number[];
  enabled: boolean;
}

interface RemoteAccessHit {
  tool: string;
  confidence: "info" | "high";
  reason: string;
  port?: number | null;
  peer?: string | null;
  logHint?: string | null;
  detectedAt: string;
}

interface RemoteAccessStatus {
  running: boolean;
  watchingTools: number;
  triggered: boolean;
}

interface Props {
  isAdvanced: boolean;
  searchQuery: string;
  enabled: boolean;
  /** Per-tool enable overrides keyed by catalogue id. Missing = on. */
  toolOverrides: Record<string, boolean> | null;
  onPatch: (patch: { enabled?: boolean; tools?: Record<string, boolean> }) => void;
  /** Controlled expand for accordion behaviour in monitoring/safeguards grids. */
  expanded?: boolean;
  onExpandedChange?: (next: boolean) => void;
}

export default function RemoteAccessMonitorSection({
  isAdvanced,
  searchQuery,
  enabled,
  toolOverrides,
  onPatch,
  expanded: expandedProp,
  onExpandedChange,
}: Props) {
  const requestConfirm = useAppConfirm();
  const { systemInfo } = useAppState();
  const [expandedLocal, setExpandedLocal] = useState(false);
  const isControlled = expandedProp !== undefined && onExpandedChange !== undefined;
  const expanded = isControlled ? expandedProp! : expandedLocal;
  const setExpanded = (updater: boolean | ((v: boolean) => boolean)) => {
    const next = typeof updater === "function" ? updater(expanded) : updater;
    if (isControlled) onExpandedChange!(next);
    else setExpandedLocal(next);
  };
  const [tools, setTools] = useState<ToolEntry[]>([]);
  const [recent, setRecent] = useState<RemoteAccessHit[]>([]);
  const [status, setStatus] = useState<RemoteAccessStatus | null>(null);
  const [rdpLockObserved, setRdpLockObserved] = useState<boolean | null>(null);
  const [applyingRdpLock, setApplyingRdpLock] = useState(false);
  const [rdpLockError, setRdpLockError] = useState<string | null>(null);
  const [rdpLockServiceDown, setRdpLockServiceDown] = useState(false);
  const [repairingService, setRepairingService] = useState(false);
  const needsElevation = systemInfo?.isAdmin !== true;

  const refreshTools = useCallback(async () => {
    try {
      const list = await invoke<ToolEntry[]>("get_remote_access_tools");
      setTools(list);
    } catch {
      setTools([]);
    }
  }, []);

  const refreshRecent = useCallback(async () => {
    try {
      const r = await invoke<RemoteAccessHit[]>("get_remote_access_recent");
      setRecent(r);
    } catch {
      setRecent([]);
    }
  }, []);

  const refreshStatus = useCallback(async () => {
    try {
      const s = await invoke<RemoteAccessStatus>("remote_access_monitor_status");
      setStatus(s);
    } catch {
      setStatus(null);
    }
  }, []);

  // Catalogue + status: refresh on mount and whenever enabled flips.
  useEffect(() => {
    if (!enabled) {
      setStatus(null);
      return;
    }
    refreshTools();
    refreshStatus();
  }, [enabled, refreshTools, refreshStatus]);

  // Recent log: poll-while-expanded (5s), slower when collapsed (30s).
  useEffect(() => {
    if (!enabled) {
      setRecent([]);
      return;
    }
    refreshRecent();
    const id = setInterval(() => {
      refreshRecent();
      refreshStatus();
    }, expanded ? 5_000 : 30_000);
    return () => clearInterval(id);
  }, [enabled, expanded, refreshRecent, refreshStatus]);

  if (searchQuery.trim()) return null;

  const onToggleTool = async (toolId: string, next: boolean) => {
    // Optimistic catalogue update + persist the override map.
    setTools((prev) => prev.map((t) => (t.id === toolId ? { ...t, enabled: next } : t)));
    onPatch({ tools: { ...(toolOverrides ?? {}), [toolId]: next } });
    try {
      await invoke("set_remote_access_tool_enabled", { toolId, enabled: next });
    } catch (err) {
      await refreshTools();
      showError(`Couldn't update ${toolId}: ${err}`);
    }
  };

  const onClearRecent = async () => {
    const accepted = await requestConfirm({
      title: "Clear recent remote-access detections?",
      description: "This removes the recent remote-session findings recorded for this app session.",
      confirmLabel: "Clear detections",
    });
    if (!accepted) return;
    try {
      await invoke("clear_remote_access_recent");
    } catch {
      /* ignore */
    }
    refreshRecent();
    refreshStatus();
  };

  const applyRdpLock = async (locked: boolean) => {
    if (needsElevation) {
      setRdpLockError("Blocked: needs-elevation. Requires an administrator to change the machine-wide RDP lock.");
      return;
    }
    if (locked) {
      const accepted = await requestConfirm({
        title: "Block incoming Remote Desktop?",
        description: "This blocks incoming RDP connections for every Windows account on this PC. You can restore access here later.",
        confirmLabel: "Block Remote Desktop",
      });
      if (!accepted) return;
    }

    setApplyingRdpLock(true);
    setRdpLockError(null);
    setRdpLockServiceDown(false);
    try {
      const observed = await applyMachineSetting({
        setting: "rdp_lock",
        value: { kind: "rdp_lock", locked },
      });
      const readBackMatches = observed.kind === "rdp_lock" && observed.locked === locked;
      if (!readBackMatches) {
        setRdpLockError("Failed: service read-back did not match the requested RDP lock state.");
        return;
      }
      setRdpLockObserved(observed.locked);
      showSuccess(observed.locked ? "Incoming Remote Desktop blocked" : "Incoming Remote Desktop restored");
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      // `svc_client::call`'s pipe-open step throws exactly this prefix when
      // WinCommanderSvc isn't installed or isn't running — offer a repair
      // path instead of surfacing the raw OS error (e.g. "os error 2").
      const serviceDown = message.toLowerCase().includes("service connect failed");
      setRdpLockServiceDown(serviceDown);
      setRdpLockError(
        serviceDown
          ? "The WinCommander system service isn't running, so this couldn't be applied."
          : message
      );
    } finally {
      setApplyingRdpLock(false);
    }
  };

  const handleRepairService = async () => {
    setRepairingService(true);
    try {
      await invoke<string>("repair_commander_service");
      setRdpLockError(null);
      setRdpLockServiceDown(false);
      showSuccess("WinCommander service repaired. Try Block/Restore again.");
    } catch (error) {
      setRdpLockError(error instanceof Error ? error.message : String(error));
    } finally {
      setRepairingService(false);
    }
  };

  // Header status tag: idle / watching N / triggered (any high-confidence hit).
  const hasHighTrip = recent.some((r) => r.confidence === "high");
  const watching = status?.watchingTools ?? tools.filter((t) => t.enabled).length;
  const headerRight = (
    <Tag
      minimal
      intent={enabled && hasHighTrip ? 'danger' : enabled ? 'success' : 'none'}
      className="font-mono"
    >
      {enabled && hasHighTrip
        ? `TRIGGERED · ${recent.filter((r) => r.confidence === "high").length}`
        : enabled
          ? `WATCHING ${watching}`
          : 'OFF'}
    </Tag>
  );

  return (
      <SectionCard
        title={isAdvanced ? "Remote Access Monitor" : "Remote control alert"}
        icon="desktop"
        headerRight={headerRight}
        armed={enabled}
      >
        <div className="flex items-start justify-between gap-4">
          <div className="flex flex-col gap-1 min-w-0">
            <p className="text-xs text-[var(--shield-text-subtle)] text-pretty max-w-[340px]">
              {isAdvanced
                ? "Alerts on active incoming remote-control sessions across known tools."
                : "Warns when someone may be controlling this PC remotely."}
            </p>
          </div>
          <Switch
            checked={enabled}
            onChange={(e) => onPatch({ enabled: e.currentTarget.checked })}
            aria-label="Enable remote access monitoring"
          />
        </div>

        <div className="mt-4 border-t border-[var(--shield-inner-border)] pt-4">
          <div className="flex items-start justify-between gap-4">
            <div className="flex flex-col gap-1 min-w-0">
              <div className="flex items-center gap-2 flex-wrap">
                <span className="text-xs font-semibold text-[var(--shield-text-primary)]">Incoming RDP access</span>
                <Tag minimal intent={rdpLockObserved === true ? "danger" : "none"} className="font-mono">
                  {rdpLockObserved === true ? "BLOCKED" : rdpLockObserved === false ? "RESTORED" : "NOT CHECKED"}
                </Tag>
                <span className="text-[10px] text-[var(--shield-text-muted)]">Machine only</span>
                {needsElevation && <span className="text-[10px] text-[var(--warn)]">Requires an administrator</span>}
              </div>
              <p className="text-xs text-[var(--shield-text-subtle)] text-pretty max-w-[420px]">
                Block or restore incoming Remote Desktop for this PC. The result is confirmed from the privileged Windows service after it applies the firewall rule.
              </p>
            </div>
            <div className="flex shrink-0 gap-2">
              <Button small intent="danger" disabled={applyingRdpLock || needsElevation} onClick={() => void applyRdpLock(true)} aria-label="Block incoming Remote Desktop">
                {applyingRdpLock ? "Applying…" : "Block RDP"}
              </Button>
              <Button small disabled={applyingRdpLock || needsElevation} onClick={() => void applyRdpLock(false)} aria-label="Restore incoming Remote Desktop">
                Restore
              </Button>
            </div>
          </div>
          {needsElevation && <p role="alert" className="mt-2 text-xs text-[var(--warn)]">Blocked: needs-elevation. Requires an administrator to change the machine-wide RDP lock.</p>}
          {rdpLockError && <p role="alert" className="mt-2 text-xs text-[var(--danger)]">{rdpLockError}</p>}
          {rdpLockServiceDown && !needsElevation && (
            <Button
              small
              className="mt-2"
              disabled={repairingService}
              onClick={() => void handleRepairService()}
              aria-label="Repair WinCommander service"
            >
              {repairingService ? "Repairing…" : "Repair service"}
            </Button>
          )}
        </div>

        {enabled && (
          <div className="mt-4 pt-4 border-t border-[var(--shield-inner-border)]">
            <button
              type="button"
              className="flex items-center justify-between w-full cursor-pointer hover:opacity-80 transition-opacity"
              onClick={() => setExpanded((v) => !v)}
              aria-label="Configure remote access monitor"
              aria-expanded={expanded}
            >
              <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
                Configure
              </span>
              <Icon
                icon={expanded ? "chevron-up" : "chevron-down"}
                size={12}
                color="var(--shield-text-muted)"
              />
            </button>

            {expanded && (
              <div className="mt-4 flex flex-col gap-5">
                {/* Tool catalogue */}
                <div className="flex flex-col gap-2">
                  <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
                    Tools watched ({watching}/{tools.length})
                  </span>
                  <div className="flex flex-col gap-1 max-h-[220px] overflow-y-auto">
                    {tools.map((t) => (
                      <div
                        key={t.id}
                        className="flex items-center justify-between gap-2 px-3 py-1.5 rounded bg-[var(--color-bg-secondary)] border border-[var(--shield-inner-border)]"
                      >
                        <span className="flex flex-col min-w-0">
                          <span className="text-[11px] text-[var(--shield-text-subtle)] truncate">
                            {t.label}
                          </span>
                          {t.ports.length > 0 && (
                            <span className="text-[10px] text-[var(--shield-text-muted)] font-mono truncate">
                              tcp/{t.ports.join(", ")}
                            </span>
                          )}
                        </span>
                        <Switch
                          checked={t.enabled}
                          onChange={(e) => onToggleTool(t.id, e.currentTarget.checked)}
                          className="!mb-0"
                          aria-label={`Watch ${t.label}`}
                        />
                      </div>
                    ))}
                  </div>
                </div>

                {/* Recent detections */}
                {recent.length > 0 && (
                  <div className="flex flex-col gap-2">
                    <div className="flex items-center justify-between">
                      <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
                        Recent ({recent.length})
                      </span>
                      <Button small minimal onClick={onClearRecent} aria-label="Clear remote access detections">
                        Clear
                      </Button>
                    </div>
                    <PrivacyEventTable title="Remote access detections" columns={["Time", "Status", "Tool", "Peer"]} rows={recent.map((r, i) => ({ id: `${r.detectedAt}-${i}`, search: `${r.confidence} ${r.tool} ${r.peer ?? ''}`, sort: [r.detectedAt, r.confidence, r.tool, r.peer ?? ''], cells: [formatRelative(r.detectedAt), r.confidence === 'high' ? 'SESSION' : 'Running', r.tool, r.peer || '—'] }))} />
                  </div>
                )}
              </div>
            )}
          </div>
        )}
      </SectionCard>
  );
}


function formatRelative(iso: string): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "";
  const diffMs = Date.now() - t;
  const diffSec = Math.max(0, Math.round(diffMs / 1000));
  if (diffSec < 60) return `${diffSec}s ago`;
  const m = Math.floor(diffSec / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  const d = Math.floor(h / 24);
  return `${d}d ago`;
}
