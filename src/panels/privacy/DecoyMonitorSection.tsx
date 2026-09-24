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
// Pro owns the watcher and canonical machine registry. The persisted Free
// setting is additive rearm intent; explicit remove/delete actions are the
// only operations that unenrol a Pro registration. The global hook sends one
// complete atomic arm request rather than a start/list/path-diff sequence.

import { Switch, Icon, Button } from "@/components/ui/bp";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useState } from "react";
import { showSuccess, showError } from "../../utils/toast";
import { DecoyMonitorIntro } from "./MonitorIntros";
import SectionCard from "../../components/shared/SectionCard";
import { useAppConfirm } from "../../components/shared/AppConfirmDialog";
import { requestDestructiveCapability } from "../../hooks/destructiveAuthz";
import PrivacyEventTable from './PrivacyEventTable';
import TierGate from "../../components/shared/TierGate";
import useEntitlements from "../../hooks/useEntitlements";
import { decoyEventLabel } from "../../lib/decoyEventPresentation";

interface DecoyInfoRow {
  path: string;
  exists: boolean;
  /** Only a WinCommander-created standard decoy may be deleted from disk. */
  standard?: boolean;
}

interface DecoyAccessRow {
  path: string;
  kind: string;
  detected_at: string;
  user_name?: string | null;
  domain?: string | null;
  sid?: string | null;
  process_name?: string | null;
  is_administrator?: boolean | null;
}

interface Props {
  isAdvanced: boolean;
  searchQuery: string;
  enabled: boolean;
  enrolledPaths: string[];
  readAuditEnabled: boolean;
  fleetAlertEnabled: boolean;
  /** The signed Fleet reporting policy owns this one switch, not the decoy
   * detector's local arm/configuration controls. */
  fleetAlertLocked?: boolean;
  onPatchDecoy: (patch: { enabled?: boolean; enrolledPaths?: string[]; readAuditEnabled?: boolean; fleetAlertEnabled?: boolean }) => Promise<void>;
  /** Controlled expand for accordion behaviour in monitoring/safeguards grids. */
  expanded?: boolean;
  onExpandedChange?: (next: boolean) => void;
}

export default function DecoyMonitorSection({
  searchQuery,
  enabled,
  enrolledPaths,
  readAuditEnabled,
  fleetAlertEnabled,
  fleetAlertLocked = false,
  onPatchDecoy,
  expanded: expandedProp,
  onExpandedChange,
}: Props) {
  const { hasPaid } = useEntitlements();
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
  const [monitorRunning, setMonitorRunning] = useState<boolean | null>(null);
  const [monitorError, setMonitorError] = useState(false);
  const [openDetectionRunning, setOpenDetectionRunning] = useState<boolean | null>(null);

  const refreshDecoys = useCallback(async () => {
    if (!hasPaid) return;
    try {
      const [list, running, audit] = await Promise.all([
        invoke<DecoyInfoRow[]>("list_decoys"),
        invoke<boolean>("decoy_monitor_status"),
        invoke<{ running: boolean }>("decoy_read_audit_status"),
      ]);
      setDecoys(list);
      setMonitorRunning(running);
      setMonitorError(false);
      setOpenDetectionRunning(audit.running);
    } catch {
      setMonitorRunning(null);
      setMonitorError(true);
      setOpenDetectionRunning(null);
    }
  }, [hasPaid]);

  const refreshRecent = useCallback(async () => {
    if (!hasPaid) return;
    try {
      const r = await invoke<DecoyAccessRow[]>("get_decoy_recent");
      setRecent(r);
    } catch {
      setRecent([]);
    }
  }, [hasPaid]);

  // Refresh on mount + whenever the settings list changes — handles
  // the case where the user just dropped standard decoys (which
  // mutates Rust's internal set independently of settings).
  useEffect(() => {
    if (!hasPaid) {
      setDecoys([]);
      return;
    }
    refreshDecoys();
  }, [hasPaid, refreshDecoys, enrolledPaths.length]);

  // A file can be removed outside WinCommander. Poll the authoritative
  // registry while this detail view is open so a missing file does not remain
  // as a stale card row until the user leaves and returns to Privacy Settings.
  useEffect(() => {
    if (!hasPaid || !enabled) return;
    const id = setInterval(refreshDecoys, expanded ? 5_000 : 30_000);
    return () => clearInterval(id);
  }, [enabled, expanded, hasPaid, refreshDecoys]);

  // Recent log: poll-while-expanded so the "Recent (3)" mini-list
  // stays current. When collapsed only refresh on settings changes
  // (count badge can be stale by up to 30s — fine).
  useEffect(() => {
    if (!hasPaid) {
      setRecent([]);
      return;
    }
    if (!enabled) {
      setRecent([]);
      return;
    }
    refreshRecent();
    const id = setInterval(refreshRecent, expanded ? 5_000 : 30_000);
    return () => clearInterval(id);
  }, [enabled, expanded, hasPaid, refreshRecent]);

  if (searchQuery.trim()) return null;

  if (!hasPaid) {
    return (
      <SectionCard title="Decoy File Monitor" icon="document">
        <TierGate
          tier="paid"
          featureLabel="Decoy File Monitor"
          fallback={(
            <p className="text-xs text-[var(--shield-text-subtle)]">
              Pro adds organisation-grade filesystem tripwires: decoy files, access auditing, and optional Fleet alerts. It is kept out of Free so everyday users are not asked to manage security bait files.
            </p>
          )}
        >
          {null}
        </TierGate>
      </SectionCard>
    );
  }

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
      await onPatchDecoy({ enrolledPaths: merged });
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
      if (enrolledPaths.some((path) => sameDecoyPath(path, picked))) {
        showError("Already enrolled.");
        return;
      }
      // Call enroll_decoy directly so list_decoys immediately returns the
      // new file. The hook reconciliation would also call enroll_decoy but
      // only after the settings update propagates — if we refresh the list
      // before that, the new file is absent. Calling it here first ensures
      // refreshDecoys() sees the enrolled file right away.
      await invoke("enroll_decoy", { path: picked });
      await onPatchDecoy({ enrolledPaths: [...enrolledPaths, picked] });
      await refreshDecoys();
      showSuccess("Decoy enrolled.");
    } catch (err) {
      showError(`Couldn't enroll decoy: ${err}`);
    }
  };

  const onRemove = async (path: string) => {
    const accepted = await requestConfirm({
      title: "Stop watching this decoy?",
      description: `${path}\n\nThis removes only the monitoring record. The file remains on disk.`,
      confirmLabel: "Stop watching",
    });
    if (!accepted) return;
    try {
      // Remove the runtime watch first. Persisting alone used to leave the
      // active watcher armed until an unrelated settings render happened.
      await invoke("remove_decoy", { path });
      await onPatchDecoy({ enrolledPaths: enrolledPaths.filter((p) => !sameDecoyPath(p, path)) });
      await refreshDecoys();
      showSuccess("Decoy monitoring record removed. The file remains on disk.");
    } catch (err) {
      showError(`Couldn't unenroll decoy: ${err}`);
    }
  };

  const onDeleteFile = async (path: string) => {
    const decoy = decoys.find((candidate) => sameDecoyPath(candidate.path, path));
    if (!decoy?.standard) {
      showError("Only WinCommander’s standard decoys can be deleted here. Stop watching a custom file instead.");
      return;
    }
    const accepted = await requestConfirm({
      title: "Delete decoy file?",
      description: `${path}\n\nThis removes the actual file from disk, not only the watch entry. This cannot be undone.`,
      confirmLabel: "Delete file",
    });
    if (!accepted) return;
    try {
      const capabilityToken = await requestDestructiveCapability(
        { command: "delete_decoy", path },
      );
      await invoke("delete_decoy", { path, capabilityToken });
      await onPatchDecoy({ enrolledPaths: enrolledPaths.filter((p) => !sameDecoyPath(p, path)) });
      await refreshDecoys();
      showSuccess("Decoy file deleted.");
    } catch (err) {
      showError(`Delete failed: ${err}`);
    }
  };

  const onClearRecent = async () => {
    const accepted = await requestConfirm({
      title: "Clear recent decoy access events?",
      description: "This removes only the recent decoy-file event history. Enrolled files and monitoring records are not changed.",
      confirmLabel: "Clear events",
    });
    if (!accepted) return;
    try {
      await invoke("clear_decoy_recent");
      await refreshRecent();
    } catch (err) {
      showError(`Couldn't clear decoy events: ${err}`);
    }
  };

  // Status pill states: idle / watching / triggered (any recent events).
  // Missing files are intentionally absent from the card. Their deletion
  // remains visible in Recent events, but a card row must describe a file the
  // user can still manage rather than a stale registration.
  const visibleDecoys = decoys.filter((decoy) => decoy.exists);
  const hasRecentTrip = recent.length > 0;
  let statusPill: React.ReactNode = null;
  if (enabled && monitorRunning !== true) {
    statusPill = (
      <span className="text-[10px] px-2 py-0.5 rounded text-[var(--color-warning)] flex-shrink-0 font-mono" role="status">
        {monitorError ? "Monitor unavailable" : monitorRunning === false ? "Not watching" : "Checking monitor…"}
      </span>
    );
  } else if (enabled && hasRecentTrip) {
    statusPill = (
      <span className="text-[10px] px-2 py-0.5 rounded bg-[var(--color-danger,#f87171)]/15 text-[var(--color-danger,#f87171)] border border-[var(--color-danger,#f87171)]/40 flex-shrink-0 font-mono">
        Triggered · {recent.length}
      </span>
    );
  } else if (enabled) {
    statusPill = (
      <span className="text-[10px] px-2 py-0.5 rounded bg-[var(--color-success)]/15 text-[var(--color-success)] border border-[var(--color-success)]/30 flex-shrink-0 font-mono">
        Watching {visibleDecoys.length}
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
        title="Decoy files"
        icon="document"
        headerRight={(
          <div className="flex items-center gap-2">
            {statusPill}
          </div>
        )}
        armed={(enabled && monitorRunning === true) || hasRecentTrip}
      >
        <div className="flex flex-col gap-3 min-w-0">
          <div className="flex items-start justify-between gap-3">
            <p className="text-xs text-[var(--shield-text-subtle)] text-pretty max-w-[420px]">
              Watches enrolled local files for changes, renames, deletion/removal, reads, and opens.
            </p>
            <button
              type="button"
              onClick={() => setShowIntro(true)}
              aria-label="How decoy file monitoring works"
              aria-expanded={showIntro}
              className="inline-grid size-6 shrink-0 place-items-center rounded-full border border-[var(--color-accent)]/30 text-[var(--color-accent)] hover:bg-[var(--color-accent)]/10 transition-colors"
            >
              <Icon icon="info-sign" size={11} />
            </button>
          </div>
          <label className="flex items-start gap-2 rounded border border-[var(--shield-inner-border)] px-3 py-2 text-[11px] text-[var(--shield-text-subtle)] cursor-pointer">
            <Switch
              checked={enabled}
              onChange={(e) => onPatchDecoy({ enabled: e.currentTarget.checked })}
              aria-label="Enable decoy file monitor"
            />
            <span>
              Monitor decoy files
              <span className="block text-[10px] text-[var(--shield-text-muted)]">
                {enabled ? "Watching enrolled files for changes and access." : "Turn on to arm enrolled decoy files."}
              </span>
            </span>
          </label>
          {enabled && visibleDecoys.length === 0 && (
            <p className="text-[11px] text-[var(--color-warning)]">
              No decoys enrolled yet — click Configure to drop the standard set.
            </p>
          )}
          {enabled && (
            <div className="grid gap-2">
              <label className="flex items-start gap-2 rounded border border-[var(--shield-inner-border)] px-3 py-2 text-[11px] text-[var(--shield-text-subtle)] cursor-pointer">
                <input
                  type="checkbox"
                  checked={readAuditEnabled}
                  onChange={(e) => onPatchDecoy({ readAuditEnabled: e.currentTarget.checked })}
                  aria-label="Detect reads and opens of decoy files"
                  className="mt-0.5 accent-[var(--color-accent)]"
                />
                <span>
                  Detect reads and opens
                  <span className="block text-[10px] text-[var(--shield-text-muted)]">
                    Uses Windows Security auditing to detect a read-only open and, when Windows provides it, the opening account and app. Requires Administrator approval.
                  </span>
                  {readAuditEnabled && openDetectionRunning !== true && (
                    <span className="block text-[10px] text-[var(--color-warning)]" role="status">
                      {openDetectionRunning === false
                        ? "Open detection is not running. Run WinCommander as Administrator and re-enable this option."
                        : "Open detection has not been confirmed."}
                    </span>
                  )}
                </span>
              </label>
              <label className={`flex items-start gap-2 rounded border border-[var(--shield-inner-border)] px-3 py-2 text-[11px] text-[var(--shield-text-subtle)] ${fleetAlertLocked ? "opacity-70" : "cursor-pointer"}`}>
                <input
                  type="checkbox"
                  checked={fleetAlertEnabled}
                  disabled={fleetAlertLocked}
                  onChange={(e) => onPatchDecoy({ fleetAlertEnabled: e.currentTarget.checked })}
                  aria-label="Notify Fleet admins about decoy access"
                  className="mt-0.5 accent-[var(--color-accent)]"
                />
                <span>
                  Notify Fleet admins
                  <span className="block text-[10px] text-[var(--shield-text-muted)]">
                    {fleetAlertLocked
                      ? "Required by your Fleet policy. Fleet receives the file path and Windows account/app when Windows records them."
                      : "Sends this decoy incident to Fleet with the file path and Windows account/app when available. The SID stays on this PC."}
                  </span>
                </span>
              </label>
            </div>
          )}
        </div>

        {enabled && (
          <div className="mt-4 pt-4 border-t border-[var(--shield-inner-border)]">
            <button
              type="button"
              className="flex items-center justify-between w-full cursor-pointer hover:opacity-80 transition-opacity"
              onClick={() => setExpanded((v) => !v)}
              aria-label="Configure decoy file monitor"
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

                {/* Enrolled list */}
                {visibleDecoys.length > 0 && (
                  <div className="flex flex-col gap-2">
                    <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
                      Enrolled ({visibleDecoys.length})
                    </span>
                    <div className="flex flex-col gap-1 max-h-[200px] overflow-y-auto">
                      {visibleDecoys.map((d) => (
                        <div
                          key={d.path}
                          className="flex items-center justify-between gap-2 px-3 py-1.5 rounded bg-[var(--color-bg-secondary)] border border-[var(--shield-inner-border)]"
                        >
                          <span className="flex items-center gap-2 min-w-0">
                            <Icon
                              icon="document"
                              size={11}
                              color="var(--shield-text-muted)"
                              title="File present"
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
                              onClick={() => { void onRemove(d.path); }}
                              title="Stop watching (file stays on disk)"
                              aria-label={`Stop watching ${shortPath(d.path)}; keep file on disk`}
                            />
                            {d.standard && (
                              <Button
                                small
                                minimal
                                icon="trash"
                                intent="danger"
                                onClick={() => { void onDeleteFile(d.path); }}
                                title="Delete WinCommander standard decoy from disk"
                                aria-label={`Delete ${shortPath(d.path)} from disk`}
                              />
                            )}
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
                      <Button small minimal onClick={onClearRecent} aria-label="Clear decoy access events">
                        Clear
                      </Button>
                    </div>
                    <PrivacyEventTable title="Decoy file events" columns={["Time", "Event", "User", "Process", "Decoy path"]} rows={recent.map((r, i) => ({ id: `${r.detected_at}-${i}`, search: `${r.kind} ${r.path} ${r.user_name ?? ""} ${r.process_name ?? ""}`, sort: [r.detected_at, r.kind, r.user_name ?? "", r.process_name ?? "", r.path], cells: [formatRelative(r.detected_at), decoyEventLabel(r.kind), r.user_name ? <span title={`${r.domain ?? ""}\\${r.user_name}${r.sid ? ` · ${r.sid}` : ""}`}>{r.domain ? `${r.domain}\\${r.user_name}` : r.user_name}{r.is_administrator ? " · Admin" : ""}</span> : "—", r.process_name ? <span className="font-mono" title={r.process_name}>{shortPath(r.process_name)}</span> : "—", <span className="font-mono" title={r.path}>{shortPath(r.path)}</span>] }))} />
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

/** Windows paths are case-insensitive and may arrive with slash variants. */
function sameDecoyPath(left: string, right: string): boolean {
  return left.replaceAll("/", "\\").toLocaleLowerCase()
    === right.replaceAll("/", "\\").toLocaleLowerCase();
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
