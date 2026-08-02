// src/panels/privacy/RansomwareMonitorSection.tsx
//
// F-3 Mass-encryption alarm — Privacy panel section card.
//
// Watches user-content directories (Documents, Pictures, Desktop,
// Downloads) for mass-modification patterns. Threshold + window are
// user-tunable but bounded. v1 just notifies; v2 will add Pro
// kill-process / dismount-vault actions when ETW gives us
// process attribution.

import { Switch, Icon, Slider, Button } from "@/components/ui/bp";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useState } from "react";
import { showError, showSuccess } from "../../utils/toast";
import { RansomwareMonitorIntro } from "./MonitorIntros";
import {
  DEFAULT_RANSOMWARE_THRESHOLD,
  DEFAULT_RANSOMWARE_WINDOW_SECONDS,
} from "../../hooks/useRansomwareMonitor";
import SectionCard from "../../components/shared/SectionCard";
import { useAppConfirm } from "../../components/shared/AppConfirmDialog";
import type { RansomwareAction } from "../../types/settings";

interface RansomwareDetection {
  count: number;
  window_seconds: number;
  sample_paths: string[];
  detected_at: string;
  // v2 (Pro ETW) attribution — absent / zero on the notify fallback path.
  pid?: number;
  image_name?: string;
  image_path?: string;
  action_taken?: string;
}

const ACTION_OPTIONS: { value: RansomwareAction; label: string; hint: string }[] = [
  { value: "monitor", label: "Alert only", hint: "Notify and name the process. No automatic action." },
  { value: "suspend", label: "Suspend", hint: "Freeze the process (reversible — resume from Task Manager)." },
  { value: "kill", label: "Kill", hint: "Terminate the process immediately (irreversible)." },
];

interface Props {
  isAdvanced: boolean;
  searchQuery: string;
  enabled: boolean;
  threshold: number;
  windowSeconds: number;
  customWatchDirs: string[];
  action: RansomwareAction;
  onPatchRansomware: (patch: {
    enabled?: boolean;
    threshold?: number;
    windowSeconds?: number;
    customWatchDirs?: string[];
    action?: RansomwareAction;
  }) => void;
  /** Controlled expand for accordion behaviour in monitoring/safeguards grids. */
  expanded?: boolean;
  onExpandedChange?: (next: boolean) => void;
}

export default function RansomwareMonitorSection({
  isAdvanced,
  searchQuery,
  enabled,
  threshold,
  windowSeconds,
  customWatchDirs,
  action,
  onPatchRansomware,
  expanded: expandedProp,
  onExpandedChange,
}: Props) {
  const requestConfirm = useAppConfirm();
  const [expandedLocal, setExpandedLocal] = useState(false);
  const isControlled = expandedProp !== undefined && onExpandedChange !== undefined;
  const expanded = isControlled ? expandedProp! : expandedLocal;
  const setExpanded = (updater: boolean | ((v: boolean) => boolean)) => {
    const next = typeof updater === 'function' ? updater(expanded) : updater;
    if (isControlled) onExpandedChange!(next);
    else setExpandedLocal(next);
  };
  const [showIntro, setShowIntro] = useState(false);
  const [recent, setRecent] = useState<RansomwareDetection[]>([]);
  const [watchedDirs, setWatchedDirs] = useState<string[]>([]);

  // Distinguish standard dirs (hardcoded, can't remove) from custom
  // ones (user-added, with remove button).
  const customSet = new Set(customWatchDirs);
  const standardWatched = watchedDirs.filter((d) => !customSet.has(d));

  const refreshRecent = useCallback(async () => {
    try {
      const r = await invoke<RansomwareDetection[]>("get_ransomware_recent");
      setRecent(r);
    } catch {
      setRecent([]);
    }
  }, []);

  const refreshWatchedDirs = useCallback(async () => {
    try {
      const dirs = await invoke<string[]>("get_ransomware_watched_dirs");
      setWatchedDirs(dirs);
    } catch {
      setWatchedDirs([]);
    }
  }, []);

  useEffect(() => {
    if (!enabled) {
      setRecent([]);
      return;
    }
    refreshRecent();
    refreshWatchedDirs();
    const id = setInterval(refreshRecent, expanded ? 5_000 : 30_000);
    return () => clearInterval(id);
  }, [enabled, expanded, refreshRecent, refreshWatchedDirs]);

  if (searchQuery.trim()) return null;

  const hasRecent = recent.length > 0;
  const onClearRecent = async () => {
    const accepted = await requestConfirm({
      title: "Clear recent ransomware alerts?",
      description: "This removes the recent mass-encryption findings recorded for this app session.",
      confirmLabel: "Clear alerts",
    });
    if (!accepted) return;
    try { await invoke("clear_ransomware_recent"); } catch { /* ignore */ }
    refreshRecent();
  };

  const onAddCustomDir = async () => {
    try {
      const picked = await openDialog({
        multiple: false,
        directory: true,
        title: "Pick a folder to watch",
      });
      if (typeof picked !== "string") return;
      if (customWatchDirs.includes(picked) || standardWatched.includes(picked)) {
        showError("Already watching this folder.");
        return;
      }
      onPatchRansomware({ customWatchDirs: [...customWatchDirs, picked] });
      showSuccess(`Now watching ${picked.split(/[/\\]/).slice(-2).join('/')}.`);
      // Refresh dir list shortly — Rust reconciles asynchronously.
      setTimeout(() => { refreshWatchedDirs(); }, 300);
    } catch (err) {
      showError(`Couldn't add folder: ${err}`);
    }
  };

  const onRemoveCustomDir = (path: string) => {
    onPatchRansomware({
      customWatchDirs: customWatchDirs.filter((p) => p !== path),
    });
    setTimeout(() => { refreshWatchedDirs(); }, 300);
  };

  let statusPill: React.ReactNode = null;
  if (enabled && hasRecent) {
    statusPill = (
      <span className="text-[10px] px-2 py-0.5 rounded bg-[var(--color-danger,#f87171)]/15 text-[var(--color-danger,#f87171)] border border-[var(--color-danger,#f87171)]/40 flex-shrink-0">
        Triggered · {recent.length}
      </span>
    );
  } else if (enabled) {
    statusPill = (
      <span className="text-[10px] px-2 py-0.5 rounded bg-[var(--color-success)]/15 text-[var(--color-success)] border border-[var(--color-success)]/30 flex-shrink-0 font-mono">
        Watching {watchedDirs.length}
      </span>
    );
  } else {
    statusPill = (
      <span className="text-[10px] px-2 py-0.5 rounded bg-[var(--color-text-muted)]/10 text-[var(--color-text-muted)] border border-[var(--color-border)] flex-shrink-0 font-mono">
        OFF
      </span>
    );
  }

  return (
    <>
      <SectionCard
        title={isAdvanced ? "Mass-encryption sentinel" : "Mass-encryption alarm"}
        icon="shield"
        headerRight={statusPill}
        armed={enabled || hasRecent}
      >
        <div className="flex items-start justify-between gap-4">
          <div className="flex flex-col gap-1 min-w-0">
            <div className="flex items-center gap-2 flex-wrap">
              <div className={`size-2 rounded-full flex-shrink-0 ${enabled ? (hasRecent ? 'bg-[var(--color-danger,#f87171)]' : 'bg-[var(--color-success)]') : 'bg-[var(--color-text-muted)]'}`} />
              <button
                type="button"
                onClick={() => setShowIntro(true)}
                aria-label="How mass-encryption detection works"
                className="text-[10px] px-1.5 py-0.5 rounded border border-[var(--color-accent)]/30 text-[var(--color-accent)] hover:bg-[var(--color-accent)]/10 transition-colors"
              >
                How it works?
              </button>
            </div>
            <p className="text-xs text-[var(--shield-text-subtle)] text-pretty max-w-[340px]">
              {isAdvanced
                ? `Watches Documents / Pictures / Desktop / Downloads for mass-modify patterns. Fires when more than ${threshold} files change within ${windowSeconds}s — the textbook ransomware signature.`
                : "Sounds the alarm if a flood of your files start changing all at once — the typical ransomware behaviour."}
            </p>
          </div>
          <Switch
            checked={enabled}
            onChange={(e) => onPatchRansomware({ enabled: e.currentTarget.checked })}
            aria-label="Enable mass-encryption sentinel"
          />
        </div>

        {enabled && (
          <div className="mt-4 pt-4 border-t border-[var(--shield-inner-border)]">
            <button
              type="button"
              className="flex items-center justify-between w-full cursor-pointer hover:opacity-80 transition-opacity"
              onClick={() => setExpanded((v) => !v)}
              aria-label="Configure mass-encryption sentinel"
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
                {/* Sensitivity sliders */}
                <div className="flex flex-col gap-3">
                  <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
                    Sensitivity
                  </span>

                  <div className="flex flex-col gap-1">
                    <div className="flex justify-between text-[11px] text-[var(--shield-text-subtle)]">
                      <span>Threshold (files)</span>
                      <span className="font-mono tabular-nums text-[var(--color-accent)]">{threshold}</span>
                    </div>
                    <Slider
                      ariaLabel="Mass-encryption file threshold"
                      min={10}
                      max={200}
                      stepSize={5}
                      labelStepSize={50}
                      value={threshold}
                      onChange={(v) => onPatchRansomware({ threshold: v })}
                      labelRenderer={(v) => `${v}`}
                    />
                    <span className="text-[10px] text-[var(--shield-text-muted)]">
                      Lower = more sensitive (more false positives). 50 catches typical ransomware
                      while ignoring most batch operations.
                    </span>
                  </div>

                  <div className="flex flex-col gap-1">
                    <div className="flex justify-between text-[11px] text-[var(--shield-text-subtle)]">
                      <span>Window (seconds)</span>
                      <span className="font-mono tabular-nums text-[var(--color-accent)]">{windowSeconds}s</span>
                    </div>
                    <Slider
                      ariaLabel="Mass-encryption detection window in seconds"
                      min={5}
                      max={120}
                      stepSize={5}
                      labelStepSize={30}
                      value={windowSeconds}
                      onChange={(v) => onPatchRansomware({ windowSeconds: v })}
                      labelRenderer={(v) => `${v}s`}
                    />
                    <span className="text-[10px] text-[var(--shield-text-muted)]">
                      How recently the changes have to happen. Shorter = stricter.
                    </span>
                  </div>

                  {(threshold !== DEFAULT_RANSOMWARE_THRESHOLD || windowSeconds !== DEFAULT_RANSOMWARE_WINDOW_SECONDS) && (
                    <div>
                      <Button
                        small
                        minimal
                        onClick={() => onPatchRansomware({
                          threshold: DEFAULT_RANSOMWARE_THRESHOLD,
                          windowSeconds: DEFAULT_RANSOMWARE_WINDOW_SECONDS,
                        })}
                      >
                        Reset to defaults
                      </Button>
                    </div>
                  )}
                </div>

                {/* Automated response (F-3 v2 — Pro ETW attribution).
                    Only the ETW path can act on a PID; the notify
                    fallback always behaves as "Alert only". */}
                <div className="flex flex-col gap-2">
                  <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
                    When ransomware is detected
                  </span>
                  <div className="grid grid-cols-3 gap-1">
                    {ACTION_OPTIONS.map((opt) => {
                      const active = action === opt.value;
                      return (
                        <button
                          key={opt.value}
                          type="button"
                          onClick={() => onPatchRansomware({ action: opt.value })}
                          title={opt.hint}
                          aria-pressed={active}
                          className={`px-2 py-1.5 rounded border text-[11px] font-medium transition-colors ${
                            active
                              ? "bg-[var(--color-accent)]/15 text-[var(--color-accent)] border-[var(--color-accent)]/50"
                              : "bg-[var(--color-bg-secondary)] text-[var(--shield-text-subtle)] border-[var(--shield-inner-border)] hover:border-[var(--color-accent)]/40"
                          }`}
                        >
                          {opt.label}
                        </button>
                      );
                    })}
                  </div>
                  <span className="text-[10px] text-[var(--shield-text-muted)]">
                    {ACTION_OPTIONS.find((o) => o.value === action)?.hint}
                    {action !== "monitor" &&
                      " Requires WinCommander Pro running as Administrator — otherwise detection falls back to alert-only."}
                  </span>
                </div>

                {/* Watched directories — standard set (read-only) +
                    user-added custom dirs (with remove button) */}
                <div className="flex flex-col gap-2">
                  <div className="flex items-center justify-between">
                    <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
                      Watching ({watchedDirs.length})
                    </span>
                    <Button small minimal icon="folder-new" onClick={onAddCustomDir}>
                      Add folder…
                    </Button>
                  </div>

                  {standardWatched.length > 0 && (
                    <div className="flex flex-col gap-1">
                      <span className="text-[9px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)] opacity-70">
                        Standard
                      </span>
                      <div className="grid grid-cols-1 sm:grid-cols-2 gap-1">
                        {standardWatched.map((d) => (
                          <div
                            key={d}
                            className="flex items-center gap-2 px-2 py-1 rounded bg-[var(--color-bg-secondary)] border border-[var(--shield-inner-border)]"
                            title={d}
                          >
                            <Icon icon="folder-close" size={11} color="var(--shield-text-muted)" />
                            <span className="text-[11px] text-[var(--shield-text-subtle)] font-mono truncate">
                              {shortPath(d)}
                            </span>
                          </div>
                        ))}
                      </div>
                    </div>
                  )}

                  {customWatchDirs.length > 0 && (
                    <div className="flex flex-col gap-1">
                      <span className="text-[9px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)] opacity-70">
                        Custom ({customWatchDirs.length})
                      </span>
                      <div className="flex flex-col gap-1">
                        {customWatchDirs.map((d) => {
                          const live = watchedDirs.includes(d);
                          return (
                            <div
                              key={d}
                              className="flex items-center justify-between gap-2 px-2 py-1 rounded bg-[var(--color-bg-secondary)] border border-[var(--shield-inner-border)]"
                              title={live ? d : `${d}\n\nFolder doesn't exist on disk yet — will be picked up automatically when it appears.`}
                            >
                              <span className="flex items-center gap-2 min-w-0">
                                <Icon
                                  icon={live ? "folder-close" : "warning-sign"}
                                  size={11}
                                  color={live ? "var(--shield-text-muted)" : "var(--color-warning)"}
                                />
                                <span className="text-[11px] text-[var(--shield-text-subtle)] font-mono truncate">
                                  {shortPath(d)}
                                </span>
                              </span>
                              <Button
                                small
                                minimal
                                icon="cross"
                                onClick={() => onRemoveCustomDir(d)}
                                title="Stop watching this folder"
                              />
                            </div>
                          );
                        })}
                      </div>
                    </div>
                  )}

                  {customWatchDirs.length === 0 && (
                    <span className="text-[10px] text-[var(--shield-text-muted)] italic">
                      Add custom folders if you keep important files outside the standard locations
                      (cloud-sync directories, OneDrive Vault, project folders, etc).
                    </span>
                  )}
                </div>

                {/* Recent detections */}
                {recent.length > 0 && (
                  <div className="flex flex-col gap-2">
                    <div className="flex items-center justify-between">
                      <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--color-danger,#f87171)]">
                        Recent ({recent.length})
                      </span>
                      <Button small minimal onClick={onClearRecent} aria-label="Clear mass-encryption alerts">
                        Clear
                      </Button>
                    </div>
                    <div className="flex flex-col gap-2 max-h-[200px] overflow-y-auto">
                      {[...recent].reverse().map((r, i) => (
                        <div
                          key={`${r.detected_at}-${i}`}
                          className="flex flex-col gap-1 px-3 py-2 rounded bg-[var(--color-bg-secondary)] border"
                          style={{ borderColor: 'var(--color-danger, #f87171)' }}
                        >
                          <div className="flex items-center justify-between gap-2">
                            <span className="text-[11px] text-[var(--shield-text-subtle)] font-medium">
                              {r.count} files in {r.window_seconds}s
                            </span>
                            <span className="text-[10px] text-[var(--shield-text-muted)] font-mono flex-shrink-0">
                              {formatRelative(r.detected_at)}
                            </span>
                          </div>
                          {r.image_name && (
                            <div className="flex items-center gap-1.5 flex-wrap">
                              <Icon icon="application" size={11} color="var(--color-danger, #f87171)" />
                              <span
                                className="text-[11px] font-mono text-[var(--shield-text-subtle)] truncate"
                                title={r.image_path || r.image_name}
                              >
                                {r.image_name}{r.pid ? ` · PID ${r.pid}` : ""}
                              </span>
                              {r.action_taken && r.action_taken !== "none" && (
                                <span
                                  className={`text-[9px] px-1.5 py-0.5 rounded font-medium uppercase tracking-wide border ${
                                    r.action_taken.endsWith("_failed")
                                      ? "bg-[var(--color-warning)]/15 text-[var(--color-warning)] border-[var(--color-warning)]/40"
                                      : "bg-[var(--color-success)]/15 text-[var(--color-success)] border-[var(--color-success)]/40"
                                  }`}
                                >
                                  {actionLabel(r.action_taken)}
                                </span>
                              )}
                            </div>
                          )}
                          {r.sample_paths.length > 0 && (
                            <div className="flex flex-col gap-0.5">
                              {r.sample_paths.slice(0, 3).map((p, j) => (
                                <span
                                  key={j}
                                  className="text-[10px] text-[var(--shield-text-muted)] font-mono truncate"
                                  title={p}
                                >
                                  · {shortPath(p)}
                                </span>
                              ))}
                              {r.sample_paths.length > 3 && (
                                <span className="text-[10px] text-[var(--shield-text-muted)]">
                                  · +{r.sample_paths.length - 3} more…
                                </span>
                              )}
                            </div>
                          )}
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </div>
            )}
          </div>
        )}
      </SectionCard>
      <RansomwareMonitorIntro isOpen={showIntro} onClose={() => setShowIntro(false)} />
    </>
  );
}

function shortPath(p: string): string {
  const norm = p.replace(/\\/g, '/');
  const parts = norm.split('/').filter(Boolean);
  if (parts.length <= 2) return p;
  return `…/${parts.slice(-2).join('/')}`;
}

function actionLabel(action: string): string {
  switch (action) {
    case "suspended": return "Suspended";
    case "killed": return "Killed";
    case "suspend_failed": return "Suspend failed";
    case "kill_failed": return "Kill failed";
    default: return action;
  }
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
