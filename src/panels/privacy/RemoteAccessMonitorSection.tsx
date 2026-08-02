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
import { showError } from "../../utils/toast";
import SectionCard from "../../components/shared/SectionCard";
import { useAppConfirm } from "../../components/shared/AppConfirmDialog";

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
                    <div className="flex flex-col gap-1 max-h-[200px] overflow-y-auto">
                      {[...recent].reverse().map((r, i) => (
                        <RecentRow key={`${r.detectedAt}-${i}`} hit={r} />
                      ))}
                    </div>
                  </div>
                )}
              </div>
            )}
          </div>
        )}
      </SectionCard>
  );
}

/** One recent-detection row. `high` = danger styling (active session);
 *  `info` = muted styling (tool merely running, no session). */
function RecentRow({ hit }: { hit: RemoteAccessHit }) {
  const isHigh = hit.confidence === "high";
  return (
    <div
      className="flex items-center justify-between gap-2 px-3 py-1.5 rounded bg-[var(--color-bg-secondary)] border"
      style={{
        borderColor: isHigh ? "var(--color-danger, #f87171)" : "var(--shield-inner-border)",
      }}
    >
      <span className="flex items-center gap-1.5 min-w-0">
        <span
          className={`text-[10px] px-1.5 py-0.5 rounded flex-shrink-0 font-mono border ${
            isHigh
              ? "bg-[var(--color-danger)]/15 text-[var(--color-danger,#f87171)] border-[var(--color-danger,#f87171)]/30"
              : "bg-[var(--color-text-muted)]/10 text-[var(--shield-text-muted)] border-[var(--shield-inner-border)]"
          }`}
        >
          {isHigh ? "SESSION" : "running"}
        </span>
        <span className="text-[11px] text-[var(--shield-text-subtle)] truncate" title={hit.tool}>
          {hit.tool}
        </span>
        {hit.peer && (
          <span className="text-[10px] text-[var(--shield-text-muted)] font-mono truncate" title={hit.peer}>
            {hit.peer}
          </span>
        )}
      </span>
      <span className="text-[10px] text-[var(--shield-text-muted)] font-mono flex-shrink-0">
        {formatRelative(hit.detectedAt)}
      </span>
    </div>
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
