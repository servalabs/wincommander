// useDecoyMonitor — atomically reconciles the paid Pro decoy monitor.
// A pooled sidecar cannot safely receive a start/list/enrol/remove sequence:
// those calls could land in different processes. One full arm request carries
// the persisted paths and switches to the single Pro monitor owner instead.

import { useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAuthMode } from "../context/AuthModeContext";
import { showWarning } from "../utils/toast";
import type { StartupProtectionOperation } from "../lib/startupProtectionReadiness";

const MAX_REARM_ATTEMPTS = 3;

export default function useDecoyMonitor(
  enabled: boolean,
  enrolledPaths: string[],
  readAuditEnabled: boolean,
  fleetAlertEnabled: boolean,
  entitlementLoading: boolean,
  onStartupRearm?: (operation: StartupProtectionOperation, succeeded: boolean) => void,
) {
  const { mode } = useAuthMode();
  const lastReconciled = useRef<string>("");
  const warnedFailures = useRef(new Set<string>());
  const warnOnce = useCallback((key: string, message: string, error: unknown) => {
    console.warn(`[useDecoyMonitor] ${key} failed:`, error);
    if (warnedFailures.current.has(key)) return;
    warnedFailures.current.add(key);
    showWarning(message, 12_000);
  }, []);

  useEffect(() => {
    // Do not treat the intentionally false pre-resolution entitlement value
    // as expiry. Once resolved, false deliberately stops any old Pro watcher.
    if (entitlementLoading || mode === "decoy") return;

    let cancelled = false;
    let retryTimer: ReturnType<typeof setTimeout> | null = null;
    let attempt = 0;
    const fingerprint = `${enabled}|${readAuditEnabled}|${fleetAlertEnabled}|${[...enrolledPaths].sort().join("\n")}`;
    if (fingerprint === lastReconciled.current) return;

    const reconcile = async () => {
      try {
        if (!enabled) {
          // This cleanup command remains available after entitlement expiry.
          // Do not manufacture a Pro-missing retry/toast for ordinary Free
          // startup; only an installed sidecar could possibly need cleanup.
          const pro = await invoke<{ installed?: boolean }>("get_pro_install_status");
          if (pro?.installed) {
            await invoke("stop_decoy_monitor");
          }
        } else {
          await invoke("start_decoy_monitor", {
            paths: enrolledPaths,
            readAuditEnabled,
            fleetAlertEnabled,
          });
        }
        if (cancelled) return;
        // Success only: a failed sidecar spawn must be retried instead of
        // being permanently hidden by a render-level fingerprint dedupe.
        lastReconciled.current = fingerprint;
        warnedFailures.current.delete("reconcile");
        if (enabled) onStartupRearm?.("decoy-monitor", true);
      } catch (error) {
        if (cancelled) return;
        warnOnce(
          "reconcile",
          enabled
            ? "Decoy monitoring could not fully arm. WinCommander will retry automatically."
            : "Decoy monitoring could not stop cleanly. WinCommander will retry automatically.",
          error,
        );
        attempt += 1;
        if (attempt < MAX_REARM_ATTEMPTS) {
          retryTimer = setTimeout(() => { void reconcile(); }, attempt * 5_000);
        } else if (enabled) {
          onStartupRearm?.("decoy-monitor", false);
        }
      }
    };

    void reconcile();
    return () => {
      cancelled = true;
      if (retryTimer) clearTimeout(retryTimer);
    };
  }, [enabled, enrolledPaths, readAuditEnabled, fleetAlertEnabled, entitlementLoading, mode, warnOnce, onStartupRearm]);
}
