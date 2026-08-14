import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Button,
  ButtonGroup,
  Card,
  Intent,
  Spinner,
  Tag,
  NonIdealState,
  HTMLTable,
  Switch,
  Callout,
  Tooltip,
} from "@/components/ui/bp";
import { AnimatePresence, motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
// staggerDelay caps per-row delay so dozens of rows never stagger for seconds.
import { staggerDelay } from "../../components/shared/AnimatedList";
import { DURATION_S, EASE } from "../../components/shared/motion";
import PanelHeader from "../../components/shared/PanelHeader";
import "./index.css";

// ─── Types mirrored from the Rust side ────────────────────────────────

type RuntimeType = "TypeA" | "TypeB" | "TypeC" | "TypeD" | "TypeE" | "Unknown";

interface DetectedRuntime {
  pid: number;
  parentPid: number | null;
  name: string;
  exePath: string | null;
  hasVisibleWindow: boolean;
  startsAtLogon: boolean;
  kind: RuntimeType;
  hideable: boolean;
  tags: string[];
}

interface ScanResult {
  runtimes: DetectedRuntime[];
  scannedAtUnixMs: number;
  totalProcesses: number;
}

interface HideEntry {
  key: string;
  hiddenAtUnixMs: number;
  applied: boolean;
  runValueRenames: Array<{ subkey: string; originalName: string; renamedTo: string }>;
  uninstallHides: Array<{ subkey: string; previousValue: number | null }>;
}

interface VisibilityState {
  version: number;
  entries: HideEntry[];
}

interface StateView {
  state: VisibilityState;
  statePath: string;
}

interface HideReport {
  key: string;
  runRenamed: number;
  uninstallHidden: number;
  shortcutsHidden: number;
  errors: string[];
}

interface RestoreReport {
  key: string;
  runRestored: number;
  uninstallRestored: number;
  shortcutsRestored: number;
  errors: string[];
}

interface BulkReport {
  keys: string[];
  reports: HideReport[];
}

// ─── Labels ───────────────────────────────────────────────────────────

const KIND_LABEL: Record<RuntimeType, string> = {
  TypeA: "service",
  TypeB: "tray",
  TypeC: "headless",
  TypeD: "hidden",
  TypeE: "visible",
  Unknown: "Unknown",
};

const KIND_DETAIL: Record<RuntimeType, string> = {
  TypeA: "Background service with a visible control surface.",
  TypeB: "Tray process paired with a backend worker.",
  TypeC: "No visible window; runs as a background daemon.",
  TypeD: "Already quiet or hidden from normal UI surfaces.",
  TypeE: "Visible desktop app process.",
  Unknown: "Signal pattern did not match a known runtime shape.",
};

const KIND_INTENT: Record<RuntimeType, Intent> = {
  TypeA: Intent.PRIMARY,
  TypeB: Intent.SUCCESS,
  TypeC: Intent.WARNING,
  TypeD: Intent.NONE,
  TypeE: Intent.NONE,
  Unknown: Intent.NONE,
};

function parentFolder(path: string): string {
  const clean = path.replace(/\\+$/, "");
  const idx = clean.lastIndexOf("\\");
  return idx > 0 ? clean.slice(0, idx) : clean;
}

// ─── Runtime Concealment Manager ──────────────────────────────────────

export function RuntimeVisibilityManager({ embedded = false, scanKey = 0 }: { embedded?: boolean; scanKey?: number }) {
  const [stateView, setStateView] = useState<StateView | null>(null);
  const [stateLoading, setStateLoading] = useState(false);
  const [globalBusy, setGlobalBusy] = useState(false);
  const [globalMessage, setGlobalMessage] = useState<string | null>(null);

  const refreshState = useCallback(async () => {
    setStateLoading(true);
    try {
      const v = await invoke<StateView>("runtime_visibility_state");
      if (!v?.state || !Array.isArray(v.state.entries)) {
        throw new Error("Runtime visibility state returned an invalid response.");
      }
      setStateView(v);
    } catch (e) {
      setGlobalMessage(`State load failed: ${e}`);
    } finally {
      setStateLoading(false);
    }
  }, []);

  useEffect(() => {
    void refreshState();
  }, [refreshState, scanKey]);

  const hiddenKeys = useMemo(() => {
    return new Set((stateView?.state.entries ?? []).map((e) => e.key.toLowerCase()));
  }, [stateView]);

  const onHide = useCallback(
    async (key: string) => {
      setGlobalMessage(null);
      try {
        const r = await invoke<HideReport>("hide_runtime", { key });
        await refreshState();
        setGlobalMessage(
          r.errors.length
            ? `Hid ${key} with ${r.errors.length} error(s): ${r.errors.join("; ")}`
            : `Hid ${key} — autostart: ${r.runRenamed}, ARP: ${r.uninstallHidden}, shortcuts: ${r.shortcutsHidden}`,
        );
      } catch (e) {
        setGlobalMessage(`Hide failed: ${e}`);
      }
    },
    [refreshState],
  );

  const onRestore = useCallback(
    async (key: string) => {
      setGlobalMessage(null);
      try {
        const r = await invoke<RestoreReport>("restore_runtime", { key });
        await refreshState();
        setGlobalMessage(
          r.errors.length
            ? `Restored ${key} with ${r.errors.length} error(s): ${r.errors.join("; ")}`
            : `Restored ${key} (Run: ${r.runRestored}, Uninstall: ${r.uninstallRestored})`,
        );
      } catch (e) {
        setGlobalMessage(`Restore failed: ${e}`);
      }
    },
    [refreshState],
  );

  const onGlobalToggle = useCallback(
    async (hidden: boolean) => {
      setGlobalBusy(true);
      setGlobalMessage(null);
      try {
        const r = await invoke<BulkReport>("set_global_runtime_visibility", { hidden });
        await refreshState();
        if (hidden) {
          setGlobalMessage(`Hide-all: ${r.keys.length} runtime(s) processed.`);
        } else {
          setGlobalMessage(`Restored ${r.keys.length} entr${r.keys.length === 1 ? "y" : "ies"}.`);
        }
      } catch (e) {
        setGlobalMessage(`Global toggle failed: ${e}`);
      } finally {
        setGlobalBusy(false);
      }
    },
    [refreshState],
  );

  const onRestoreAll = useCallback(async () => {
    setGlobalBusy(true);
    setGlobalMessage(null);
    try {
      const r = await invoke<RestoreReport[]>("restore_all_runtimes");
      await refreshState();
      const errs = r.flatMap((x) => x.errors);
      setGlobalMessage(
        errs.length
          ? `Restore-all: ${r.length} entr${r.length === 1 ? "y" : "ies"} with ${errs.length} error(s).`
          : `Restored ${r.length} entr${r.length === 1 ? "y" : "ies"}.`,
      );
    } catch (e) {
      setGlobalMessage(`Restore-all failed: ${e}`);
    } finally {
      setGlobalBusy(false);
    }
  }, [refreshState]);

  // Embedded view (Maintenance → Startup & drivers → System Managers →
  // Conceal tab): show only the running processes, with a compact toolbar
  // above. The HKCU-only / manifest-path chrome only renders in the
  // standalone Runtime Visibility panel where power users want that detail;
  // the embedded tab kept it as noise.
  const toolbar = (
    <div className="flex items-center justify-end gap-3 flex-wrap">
      {stateView && stateView.state.entries.length > 0 && (
        <span className="wc-muted" style={{ fontSize: 11 }}>
          Hidden: <strong>{stateView.state.entries.length}</strong>
        </span>
      )}
      <ButtonGroup>
        <Tooltip content="Hide all hideable backend runtimes (HKCU only)">
          <Button
            small
            icon="eye-off"
            intent={Intent.PRIMARY}
            onClick={() => onGlobalToggle(true)}
            loading={globalBusy}
            disabled={globalBusy}
          >
            Hide all
          </Button>
        </Tooltip>
        <Tooltip content="Restore every entry currently in the manifest">
          <Button
            small
            icon="reset"
            onClick={onRestoreAll}
            loading={globalBusy}
            disabled={globalBusy || hiddenKeys.size === 0}
          >
            Restore all
          </Button>
        </Tooltip>
        {!embedded && <Button
          small
          icon="refresh"
          minimal
          onClick={refreshState}
          loading={stateLoading}
          disabled={stateLoading}
        />}
      </ButtonGroup>
    </div>
  );

  const body = (
    <div className="flex flex-col gap-3">
        {embedded && (
          <div className="system-manager-hint">
            <span>Conceal hides supported runtimes from user-facing Windows surfaces while preserving control here.</span>
          </div>
        )}
        {embedded ? (
          toolbar
        ) : (
          <Card>
            <div className="flex items-center justify-between gap-4 flex-wrap">
              <div style={{ minWidth: 0, flex: 1 }}>
                <h3 className="m-0">Backend Runtime Visibility</h3>
                <p className="wc-muted m-0 mt-1" style={{ maxWidth: 760 }}>
                  Hide installed backend apps from Settings → Apps and disable their
                  autostart entries while leaving the process running. Only HKCU
                  (current-user) writes happen in this build — HKLM / service control
                  / window suppression land in a later phase.
                </p>
                {stateView && (
                  <p className="wc-muted m-0 mt-1" style={{ fontSize: 11 }}>
                    Manifest: <code>{stateView.statePath}</code> · currently hidden:{" "}
                    <strong>{stateView.state.entries.length}</strong>
                  </p>
                )}
              </div>
              {toolbar}
            </div>
          </Card>
        )}

        {globalMessage && (
          <Callout
            intent={globalMessage.toLowerCase().includes("fail") ? Intent.DANGER : Intent.PRIMARY}
            icon="info-sign"
          >
            {globalMessage}
          </Callout>
        )}

        {/* Concealment is purely about hiding running backend runtimes. The
            standalone Services and Scheduled-Tasks managers (own tabs in the
            System Managers card, now under Maintenance → Startup & drivers)
            own those domains, so the old read-only Services / Tasks sub-tabs
            were removed from here as duplicates. */}
        <RuntimesPanel
          hiddenKeys={hiddenKeys}
          onHide={onHide}
          onRestore={onRestore}
          scanKey={scanKey}
          hideScanAction={embedded}
        />
      </div>
  );

  if (embedded) {
    return body;
  }

  return (
    <div className="panel-container">
      <PanelHeader
        title="Runtime Visibility"
        description="Hide or restore development runtimes and environments from Windows detection."
      />
      {body}
    </div>
  );
}

export default function RuntimeVisibilityPanel() {
  return <RuntimeVisibilityManager />;
}

// ─── Runtimes tab ─────────────────────────────────────────────────────

function RuntimesPanel({
  hiddenKeys,
  onHide,
  onRestore,
  scanKey,
  hideScanAction,
}: {
  hiddenKeys: Set<string>;
  onHide: (key: string) => Promise<void>;
  onRestore: (key: string) => Promise<void>;
  scanKey: number;
  hideScanAction: boolean;
}) {
  const [result, setResult] = useState<ScanResult | null>(null);
  const [scanning, setScanning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [onlyHideable, setOnlyHideable] = useState(true);
  const completedScanKey = useRef<number | undefined>(undefined);

  const scan = useCallback(async () => {
    setScanning(true);
    setError(null);
    try {
      const r = await invoke<ScanResult>("scan_runtimes");
      if (!r || !Array.isArray(r.runtimes)) {
        throw new Error("Runtime scan returned an invalid response.");
      }
      setResult(r);
    } catch (e) {
      setError(String(e));
    } finally {
      setScanning(false);
    }
  }, []);

  useEffect(() => {
    if (completedScanKey.current !== scanKey) {
      completedScanKey.current = scanKey;
      void scan();
    }
  }, [scan, scanKey]);

  const filtered = useMemo(() => {
    if (!result) return [];
    return onlyHideable ? result.runtimes.filter((r) => r.hideable) : result.runtimes;
  }, [result, onlyHideable]);

  const summary = useMemo(() => {
    if (!result) return null;
    const counts: Partial<Record<RuntimeType, number>> = {};
    for (const r of result.runtimes) counts[r.kind] = (counts[r.kind] ?? 0) + 1;
    return counts;
  }, [result]);

  if (error) {
    return (
      <Card>
        <NonIdealState
          icon="error"
          title="Scan failed"
          description={error}
          action={hideScanAction ? undefined : <Button onClick={scan}>Retry</Button>}
        />
      </Card>
    );
  }

  return (
    <div className="flex flex-col gap-3 mt-3">
      <Card>
        <div className="flex items-center gap-6 flex-wrap">
          {!hideScanAction && <Button icon="refresh" intent={Intent.PRIMARY} onClick={scan} loading={scanning}>
            Scan
          </Button>}
          {result && (
            <>
              <div>
                <div className="wc-muted" style={{ fontSize: 11 }}>Total processes</div>
                <div style={{ fontSize: 18, fontWeight: 600 }}>{result.totalProcesses}</div>
              </div>
              <div>
                <div className="wc-muted" style={{ fontSize: 11 }}>Surfaced</div>
                <div style={{ fontSize: 18, fontWeight: 600 }}>{result.runtimes.length}</div>
              </div>
              {summary && (
                <div className="flex items-center gap-2 flex-wrap">
                  {(Object.keys(KIND_LABEL) as RuntimeType[]).map((k) =>
                    summary[k] ? (
                      <Tag key={k} intent={KIND_INTENT[k]} minimal>
                        {KIND_LABEL[k]}: {summary[k]}
                      </Tag>
                    ) : null,
                  )}
                </div>
              )}
            </>
          )}
          <div className="ml-auto">
            <Switch
              checked={onlyHideable}
              onChange={(e) => setOnlyHideable(e.currentTarget.checked)}
              label="Hideable only"
              style={{ marginBottom: 0 }}
            />
          </div>
        </div>
      </Card>

      {scanning && !result && (
        <Card>
          <div className="flex items-center gap-3">
            <Spinner size={20} />
            <span>Enumerating processes, windows, and autostart entries…</span>
          </div>
        </Card>
      )}

      {result && (
        <Card style={{ padding: 0, overflowX: "auto", overflowY: "auto", maxHeight: 360 }}>
          <HTMLTable striped interactive style={{ width: "100%" }}>
            <thead>
              <tr>
                <th>Process</th>
                <th>PID</th>
                <th>Type + signal</th>
                <th>Path</th>
                <th style={{ width: 140 }}>Action</th>
              </tr>
            </thead>
            {/* AnimatePresence wraps tbody so rows exit before DOM removal.
                motion.tr provides staggered fade+rise on scan result entrance —
                dozens of rows previously appeared with zero animation. */}
            <AnimatePresence initial={false}>
              <tbody>
                {filtered.length === 0 ? (
                  <tr key="empty">
                    <td colSpan={5}>
                      <NonIdealState
                        icon="search"
                        title="Nothing to show"
                        description={
                          onlyHideable
                            ? "No backend-capable runtimes detected. Toggle off ‘Hideable only’."
                            : "No runtimes detected."
                        }
                      />
                    </td>
                  </tr>
                ) : (
                  filtered.map((r, idx) => {
                    const keyL = r.name.toLowerCase();
                    const isHidden = hiddenKeys.has(keyL);
                    return (
                      // Staggered fade+rise: each row enters sequentially,
                      // delay capped by staggerDelay so large lists never drag.
                      // opacity+y only — no width/height reflow.
                      <motion.tr
                        key={r.pid}
                        initial={{ opacity: 0, y: 4 }}
                        animate={{ opacity: 1, y: 0 }}
                        exit={{ opacity: 0 }}
                        transition={{
                          delay: staggerDelay(idx),
                          duration: DURATION_S.normal,
                          ease: EASE.enter,
                        }}
                      >
                        <td>
                          <span style={{ fontWeight: 500 }}>{r.name}</span>
                          {isHidden && <span className="runtime-visibility-state is-hidden">Concealed</span>}
                        </td>
                        <td>
                          <code>{r.pid}</code>
                          {r.parentPid != null && (
                            <span className="wc-muted" style={{ marginLeft: 6, fontSize: 11 }}>
                              ← {r.parentPid}
                            </span>
                          )}
                        </td>
                        <td>
                          <div className="runtime-signal-cell">
                            <Tag intent={KIND_INTENT[r.kind]} minimal title={KIND_DETAIL[r.kind]}>
                              {KIND_LABEL[r.kind]}
                            </Tag>
                            <div className="runtime-signal-tags">
                              {r.tags.slice(0, 2).map((t) => (
                                <Tag key={t} minimal title={t}>
                                  {t}
                                </Tag>
                              ))}
                              {r.tags.length > 2 && <Tag minimal title={r.tags.slice(2).join(", ")}>+{r.tags.length - 2}</Tag>}
                              {r.hasVisibleWindow && (
                                <Tag minimal intent={Intent.PRIMARY}>
                                  window
                                </Tag>
                              )}
                            </div>
                          </div>
                        </td>
                        <td className="wc-muted" style={{ fontSize: 11, fontFamily: "monospace" }}>
                          <Tooltip content={r.exePath ?? "—"} compact>
                            <button
                              type="button"
                              className="system-manager-path"
                              disabled={!r.exePath}
                              aria-label={`Open folder containing ${r.name}`}
                              onClick={() => r.exePath && invoke("open_path", { path: parentFolder(r.exePath) }).catch(() => {})}
                            >
                              {r.exePath ?? "—"}
                            </button>
                          </Tooltip>
                        </td>
                        <td>
                          {/* AnimatePresence + mode="wait" fades the Hide button
                              out before Restore fades in (and vice versa) so the
                              action swap doesn’t snap. No flourish on hide — enter/exit only. */}
                          <AnimatePresence mode="wait" initial={false}>
                            {isHidden ? (
                              <motion.span
                                key="restore"
                                initial={{ opacity: 0 }}
                                animate={{ opacity: 1 }}
                                exit={{ opacity: 0 }}
                                transition={{ duration: DURATION_S.fast, ease: EASE.standard }}
                                style={{ display: "inline-block" }}
                              >
                                <Button small icon="eye-open" onClick={() => onRestore(r.name)} aria-label={`Restore ${r.name}`}>
                                  Restore
                                </Button>
                              </motion.span>
                            ) : r.hideable ? (
                              <motion.span
                                key="hide"
                                initial={{ opacity: 0 }}
                                animate={{ opacity: 1 }}
                                exit={{ opacity: 0 }}
                                transition={{ duration: DURATION_S.fast, ease: EASE.standard }}
                                style={{ display: "inline-block" }}
                              >
                                <Button
                                  small
                                  icon="eye-off"
                                  intent={Intent.WARNING}
                                  onClick={() => onHide(r.name)}
                                  aria-label={`Hide ${r.name}`}
                                >
                                  Hide
                                </Button>
                              </motion.span>
                            ) : (
                              <motion.span
                                key="none"
                                initial={{ opacity: 0 }}
                                animate={{ opacity: 1 }}
                                exit={{ opacity: 0 }}
                                transition={{ duration: DURATION_S.fast, ease: EASE.standard }}
                                className="wc-muted"
                                style={{ fontSize: 11, display: "inline-block" }}
                              >
                                —
                              </motion.span>
                            )}
                          </AnimatePresence>
                        </td>
                      </motion.tr>
                    );
                  })
                )}
              </tbody>
            </AnimatePresence>
          </HTMLTable>
        </Card>
      )}
    </div>
  );
}

