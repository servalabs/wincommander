// src/panels/privacy/DecoyMonitorSection.tsx
//
// F-2 File Access Monitor — Privacy panel section card.
//
// Same progressive-disclosure pattern as the F-1 clipboard guard:
// default view = single ON/OFF + status pill. Expanding "Configure"
// reveals:
//   - Drop standard decoys button (one-click)
//   - Enrolled decoy list (path + exists indicator + per-row remove/delete)
//   - Add custom decoy by path (file picker via plugin-dialog)
//   - Recent access events (last 10, in-memory ring on Rust side)
//
// The runtime authority for which paths are watched lives in
// `decoy_monitor::WATCHED_DECOYS`; the persisted source-of-truth is
// `appSettings.ideal.privacy.decoyMonitor.enrolledPaths`. The global
// `useDecoyMonitor` hook reconciles the two on every settings change.

import { Switch, Icon, Button } from "@/components/ui/bp";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useState } from "react";
import { showSuccess, showError } from "../../utils/toast";
import { DecoyMonitorIntro } from "./MonitorIntros";
import SectionCard from "../../components/shared/SectionCard";
import { useAppConfirm } from "../../components/shared/AppConfirmDialog";

interface DecoyInfoRow {
  path: string;
  exists: boolean;
}

interface DecoyAccessRow {
  path: string;
  kind: string;
  detected_at: string;
}

interface LastAccessStatus {
  enabled: boolean;
  raw_value: number;
  system_managed: boolean;
}

interface Props {
  isAdvanced: boolean;
  searchQuery: string;
  enabled: boolean;
  enrolledPaths: string[];
  onPatchDecoy: (patch: { enabled?: boolean; enrolledPaths?: string[] }) => void;
  /** Controlled expand for accordion behaviour in monitoring/safeguards grids. */
  expanded?: boolean;
  onExpandedChange?: (next: boolean) => void;
}

export default function DecoyMonitorSection({
  isAdvanced,
  searchQuery,
  enabled,
  enrolledPaths,
  onPatchDecoy,
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
  const [decoys, setDecoys] = useState<DecoyInfoRow[]>([]);
  const [recent, setRecent] = useState<DecoyAccessRow[]>([]);
  const [lastAccessStatus, setLastAccessStatus] = useState<LastAccessStatus | null>(null);
  const [enablingLastAccess, setEnablingLastAccess] = useState(false);

  const refreshDecoys = useCallback(async () => {
    try {
      const list = await invoke<DecoyInfoRow[]>("list_decoys");
      setDecoys(list);
    } catch {
      setDecoys([]);
    }
  }, []);

  const refreshRecent = useCallback(async () => {
    try {
      const r = await invoke<DecoyAccessRow[]>("get_decoy_recent");
      setRecent(r);
    } catch {
      setRecent([]);
    }
  }, []);

  const refreshLastAccessStatus = useCallback(async () => {
    try {
      const s = await invoke<LastAccessStatus>("get_last_access_tracking_status");
      setLastAccessStatus(s);
    } catch {
      setLastAccessStatus(null);
    }
  }, []);

  // Refresh on mount + whenever the settings list changes — handles
  // the case where the user just dropped standard decoys (which
  // mutates Rust's internal set independently of settings).
  useEffect(() => {
    refreshDecoys();
  }, [refreshDecoys, enrolledPaths.length]);

  // Read-detection status — only relevant when the section is enabled
  // and the user has expanded Configure. Re-check on every expand so
  // a fresh "fsutil set" elsewhere is reflected.
  useEffect(() => {
    if (!enabled || !expanded) return;
    refreshLastAccessStatus();
  }, [enabled, expanded, refreshLastAccessStatus]);

  // Recent log: poll-while-expanded so the "Recent (3)" mini-list
  // stays current. When collapsed only refresh on settings changes
  // (count badge can be stale by up to 30s — fine).
  useEffect(() => {
    if (!enabled) {
      setRecent([]);
      return;
    }
    refreshRecent();
    const id = setInterval(refreshRecent, expanded ? 5_000 : 30_000);
    return () => clearInterval(id);
  }, [enabled, expanded, refreshRecent]);

  if (searchQuery.trim()) return null;

  const onDropStandard = async () => {
    try {
      const created = await invoke<string[]>("drop_standard_decoys");
      if (created.length === 0) {
        showError("All standard decoy filenames already exist — would have clobbered real files. None created.");
        return;
      }
      // Persist the freshly-enrolled paths into settings so they
      // survive a restart. Merge with existing enrolledPaths to avoid
      // erasing any previously-added custom paths.
      const merged = Array.from(new Set([...enrolledPaths, ...created]));
      onPatchDecoy({ enrolledPaths: merged });
      await refreshDecoys();
      showSuccess(`Dropped ${created.length} decoy${created.length === 1 ? '' : 's'} into Documents + Desktop.`);
    } catch (err) {
      showError(`Couldn't drop decoys: ${err}`);
    }
  };

  const onAddCustom = async () => {
    try {
      const picked = await openDialog({
        multiple: false,
        directory: false,
        title: "Pick a file to enroll as a decoy",
      });
      if (typeof picked !== "string") return;
      if (enrolledPaths.includes(picked)) {
        showError("Already enrolled.");
        return;
      }
      // Call enroll_decoy directly so list_decoys immediately returns the
      // new file. The hook reconciliation would also call enroll_decoy but
      // only after the settings update propagates — if we refresh the list
      // before that, the new file is absent. Calling it here first ensures
      // refreshDecoys() sees the enrolled file right away.
      await invoke("enroll_decoy", { path: picked });
      onPatchDecoy({ enrolledPaths: [...enrolledPaths, picked] });
      await refreshDecoys();
      showSuccess("Decoy enrolled.");
    } catch (err) {
      showError(`Couldn't enroll decoy: ${err}`);
    }
  };

  const onRemove = (path: string) => {
    onPatchDecoy({ enrolledPaths: enrolledPaths.filter((p) => p !== path) });
  };

  const onDeleteFile = async (path: string) => {
    const accepted = await requestConfirm({
      title: "Delete decoy file?",
      description: `${path}\n\nThis removes the actual file from disk, not only the watch entry. This cannot be undone.`,
      confirmLabel: "Delete file",
    });
    if (!accepted) return;
    try {
      await invoke("delete_decoy", { path });
      onPatchDecoy({ enrolledPaths: enrolledPaths.filter((p) => p !== path) });
      await refreshDecoys();
      showSuccess("Decoy file deleted.");
    } catch (err) {
      showError(`Delete failed: ${err}`);
    }
  };

  const onClearRecent = async () => {
    const accepted = await requestConfirm({
      title: "Clear recent decoy access events?",
      description: "This removes the recent decoy-file access events recorded for this app session.",
      confirmLabel: "Clear events",
    });
    if (!accepted) return;
    try {
      await invoke("clear_decoy_recent");
    } catch { /* ignore */ }
    refreshRecent();
  };

  const onEnableLastAccess = async () => {
    setEnablingLastAccess(true);
    try {
      await invoke("enable_last_access_tracking");
      await refreshLastAccessStatus();
      showSuccess("Read detection enabled. NTFS will update file access times — decoys now fire on read.");
    } catch (err) {
      showError(`Couldn't enable read detection: ${err}`);
    } finally {
      setEnablingLastAccess(false);
    }
  };

  // Status pill states: idle / watching / triggered (any recent events).
  const hasRecentTrip = recent.length > 0;
  let statusPill: React.ReactNode = null;
  if (enabled && hasRecentTrip) {
    statusPill = (
      <span className="text-[10px] px-2 py-0.5 rounded bg-[var(--color-danger,#f87171)]/15 text-[var(--color-danger,#f87171)] border border-[var(--color-danger,#f87171)]/40 flex-shrink-0 font-mono">
        Triggered · {recent.length}
      </span>
    );
  } else if (enabled) {
    statusPill = (
      <span className="text-[10px] px-2 py-0.5 rounded bg-[var(--color-success)]/15 text-[var(--color-success)] border border-[var(--color-success)]/30 flex-shrink-0 font-mono">
        Watching {decoys.length}
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
        title={isAdvanced ? "Decoy File Monitor" : "Honeypot files"}
        icon="document"
        headerRight={statusPill}
        armed={enabled || hasRecentTrip}
      >
        <div className="flex items-start justify-between gap-4">
          <Switch
            checked={enabled}
            onChange={(e) => onPatchDecoy({ enabled: e.currentTarget.checked })}
          />
          <div className="flex flex-col gap-1 min-w-0">
            <div className="flex items-center gap-2 flex-wrap">
              <button
                type="button"
                onClick={() => setShowIntro(true)}
                className="text-[10px] px-1.5 py-0.5 rounded border border-[var(--color-accent)]/30 text-[var(--color-accent)] hover:bg-[var(--color-accent)]/10 transition-colors"
              >
                How it works?
              </button>
            </div>
            <p className="text-xs text-[var(--shield-text-subtle)] text-pretty max-w-[340px]">
              {isAdvanced
                ? "Drops plausibly-named decoy files (passwords.txt, bitcoin-wallet.txt, aws-credentials.csv) into Documents + Desktop. Any modify / rename / delete fires a danger toast — early warning that malware or a person is scanning for sensitive files."
                : "Plants fake 'sensitive' files. If anything reads or changes them, you get a warning — even before the malware finds your real files."}
            </p>
            {enabled && decoys.length === 0 && (
              <p className="text-[11px] text-[var(--color-warning)] mt-1">
                No decoys enrolled yet — click Configure to drop the standard set.
              </p>
            )}
          </div>
        </div>

        {enabled && (
          <div className="mt-4 pt-4 border-t border-[var(--shield-inner-border)]">
            <button
              type="button"
              className="flex items-center justify-between w-full cursor-pointer hover:opacity-80 transition-opacity"
              onClick={() => setExpanded((v) => !v)}
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
                {/* Add decoys */}
                <div className="flex flex-col gap-2">
                  <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
                    Add decoys
                  </span>
                  <div className="flex items-center gap-2 flex-wrap">
                    <Button small minimal icon="plus" onClick={onDropStandard}>
                      Drop standard set
                    </Button>
                    <Button small minimal icon="document-open" onClick={onAddCustom}>
                      Pick file…
                    </Button>
                    <span className="text-[10px] text-[var(--shield-text-muted)] ml-1">
                      Standard set: 5 files in Documents + Desktop. Skips paths that already exist.
                    </span>
                  </div>
                </div>

                {/* Read detection status — atime polling only fires
                    on read access if NTFS last-access tracking is on. */}
                {lastAccessStatus && (
                  <div className="flex flex-col gap-2">
                    <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
                      Read detection
                    </span>
                    {lastAccessStatus.enabled ? (
                      <div className="flex items-center gap-2 px-3 py-2 rounded bg-[var(--color-success)]/10 border border-[var(--color-success)]/30">
                        <Icon icon="tick-circle" size={12} color="var(--color-success)" />
                        <span className="text-[11px] text-[var(--shield-text-subtle)]">
                          Active. Decoys also fire when something just <i>opens</i> them, not only on modify/delete.
                        </span>
                      </div>
                    ) : (
                      <div className="flex flex-col gap-2 px-3 py-2 rounded bg-[var(--color-warning)]/10 border border-[var(--color-warning)]/30">
                        <div className="flex items-center gap-2">
                          <Icon icon="warning-sign" size={12} color="var(--color-warning)" />
                          <span className="text-[11px] text-[var(--shield-text-subtle)]">
                            Read access detection is OFF — decoys will only fire on modify/rename/delete.
                          </span>
                        </div>
                        <p className="text-[10px] text-[var(--shield-text-muted)] leading-snug">
                          {isAdvanced
                            ? "Windows currently has DisableLastAccess = "
                              + lastAccessStatus.raw_value
                              + (lastAccessStatus.system_managed ? " (system-managed)." : " (user-set).")
                              + " Enabling sets the registry value system-wide via fsutil — small NTFS write-amplification cost (microseconds per file open) in exchange for honeypot read detection."
                            : "Windows is configured to skip recording when files are read. Turn this on to catch malware that reads your files without modifying them."}
                        </p>
                        <div>
                          <Button
                            small
                            intent="warning"
                            icon="key"
                            loading={enablingLastAccess}
                            onClick={onEnableLastAccess}
                          >
                            Enable read detection (admin)
                          </Button>
                        </div>
                      </div>
                    )}
                  </div>
                )}

                {/* Enrolled list */}
                {decoys.length > 0 && (
                  <div className="flex flex-col gap-2">
                    <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
                      Enrolled ({decoys.length})
                    </span>
                    <div className="flex flex-col gap-1 max-h-[200px] overflow-y-auto">
                      {decoys.map((d) => (
                        <div
                          key={d.path}
                          className="flex items-center justify-between gap-2 px-3 py-1.5 rounded bg-[var(--color-bg-secondary)] border border-[var(--shield-inner-border)]"
                        >
                          <span className="flex items-center gap-2 min-w-0">
                            <Icon
                              icon={d.exists ? "document" : "warning-sign"}
                              size={11}
                              color={d.exists ? "var(--shield-text-muted)" : "var(--color-warning)"}
                              title={d.exists ? "File present" : "File missing — will fire if recreated"}
                            />
                            <span
                              className="text-[11px] text-[var(--shield-text-subtle)] font-mono truncate"
                              title={d.path}
                            >
                              {shortPath(d.path)}
                            </span>
                          </span>
                          <span className="flex items-center gap-1 flex-shrink-0">
                            <Button
                              small
                              minimal
                              icon="cross"
                              onClick={() => onRemove(d.path)}
                              title="Stop watching (file stays on disk)"
                            />
                            <Button
                              small
                              minimal
                              icon="trash"
                              intent="danger"
                              onClick={() => onDeleteFile(d.path)}
                              title="Delete file from disk"
                            />
                          </span>
                        </div>
                      ))}
                    </div>
                  </div>
                )}

                {/* Recent access */}
                {recent.length > 0 && (
                  <div className="flex flex-col gap-2">
                    <div className="flex items-center justify-between">
                      <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--color-danger,#f87171)]">
                        Recent ({recent.length})
                      </span>
                      <Button small minimal onClick={onClearRecent}>
                        Clear
                      </Button>
                    </div>
                    <div className="flex flex-col gap-1 max-h-[160px] overflow-y-auto">
                      {[...recent].reverse().map((r, i) => (
                        <div
                          key={`${r.detected_at}-${i}`}
                          className="flex items-center justify-between gap-2 px-3 py-1.5 rounded bg-[var(--color-bg-secondary)] border"
                          style={{ borderColor: 'var(--color-danger, #f87171)' }}
                        >
                          <span className="flex items-center gap-1.5 min-w-0">
                            <span
                              className="text-[10px] px-1.5 py-0.5 rounded bg-[var(--color-danger)]/15 text-[var(--color-danger,#f87171)] border border-[var(--color-danger,#f87171)]/30 flex-shrink-0 font-mono"
                            >
                              {r.kind}
                            </span>
                            <span
                              className="text-[11px] text-[var(--shield-text-subtle)] font-mono truncate"
                              title={r.path}
                            >
                              {shortPath(r.path)}
                            </span>
                          </span>
                          <span className="text-[10px] text-[var(--shield-text-muted)] font-mono flex-shrink-0">
                            {formatRelative(r.detected_at)}
                          </span>
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
      <DecoyMonitorIntro isOpen={showIntro} onClose={() => setShowIntro(false)} />
    </>
  );
}

function shortPath(p: string): string {
  // Show last two path segments — full path lives in the title attr.
  const norm = p.replace(/\\/g, '/');
  const parts = norm.split('/').filter(Boolean);
  if (parts.length <= 2) return p;
  return `…/${parts.slice(-2).join('/')}`;
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
