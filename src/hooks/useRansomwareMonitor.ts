// src/hooks/useRansomwareMonitor.ts
//
// useRansomwareMonitor — drives the F-3 mass-modify watcher off
// settings. Same shape as the other monitor hooks: a single global
// hook reads enabled / threshold / windowSeconds and reconciles the
// Rust runtime state.
//
// Rust commands:
//   start_ransomware_monitor  — idempotent
//   stop_ransomware_monitor
//   set_ransomware_config      — pushes threshold + window to runtime
//
// On every settings change we sync config first (so the next event
// uses the fresh thresholds), then start/stop the watcher.

import { useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { RansomwareAction } from "../types/settings";
import { showWarning } from "../utils/toast";
import type { StartupProtectionOperation } from "../lib/startupProtectionReadiness";

export const DEFAULT_RANSOMWARE_THRESHOLD = 50;
export const DEFAULT_RANSOMWARE_WINDOW_SECONDS = 30;
export const DEFAULT_RANSOMWARE_ALERT_COOLDOWN_SECONDS = 300;
export const DEFAULT_RANSOMWARE_ATTRIBUTION_MIN_FILES = 5;
// Suspend-by-default per roadmap (F-3 v2): reversible, names the culprit.
export const DEFAULT_RANSOMWARE_ACTION: RansomwareAction = "suspend";

export default function useRansomwareMonitor(
  enabled: boolean,
  hasPaid: boolean,
  threshold: number,
  windowSeconds: number,
  alertCooldownSeconds: number,
  attributionMinFiles: number,
  customWatchDirs: string[],
  action: RansomwareAction,
  onStartupRearm?: (operation: StartupProtectionOperation, succeeded: boolean) => void,
) {
  const warnedFailures = useRef(new Set<string>());
  const warnOnce = useCallback((key: string, message: string, err: unknown) => {
    console.warn(`[useRansomwareMonitor] ${key} failed:`, err);
    if (warnedFailures.current.has(key)) return;
    warnedFailures.current.add(key);
    showWarning(message, 12_000);
  }, []);
  const watchDirsJson = JSON.stringify(customWatchDirs);

  // Reconcile in one ordered transaction: config, folders, then start/stop.
  // Separate effects raced during startup and could briefly arm ETW with old
  // defaults or an incomplete directory set.
  useEffect(() => {
    let cancelled = false;
    let retryTimer: ReturnType<typeof setTimeout> | null = null;
    let attempt = 0;
    const reconcile = async () => {
      try {
        await invoke("set_ransomware_config", {
          config: {
            threshold,
            windowSeconds,
            alertCooldownSeconds,
            // The basic alarm remains Free. PID attribution and automatic
            // suspend/kill are Pro controls, so stale paid settings must not
            // be replayed after a licence expires.
            attributionMinFiles: hasPaid ? attributionMinFiles : DEFAULT_RANSOMWARE_ATTRIBUTION_MIN_FILES,
            action: hasPaid ? action : "monitor",
          },
        });
        if (cancelled) return;
        await invoke("set_ransomware_watch_dirs", {
          dirs: JSON.parse(watchDirsJson) as string[],
        });
        if (cancelled) return;
        await invoke(enabled ? "start_ransomware_monitor" : "stop_ransomware_monitor");
        if (cancelled) return;
        warnedFailures.current.delete("reconcile");
        if (enabled) onStartupRearm?.("ransomware-monitor", true);
      } catch (err) {
        warnOnce(
          "reconcile",
          enabled
            ? "Mass-encryption protection could not fully arm. WinCommander will retry automatically."
            : "Mass-encryption protection could not stop cleanly. WinCommander will retry automatically.",
          err,
        );
        attempt += 1;
        if (!cancelled && attempt < 3) {
          retryTimer = setTimeout(() => { void reconcile(); }, attempt * 5_000);
        } else if (!cancelled && enabled) {
          onStartupRearm?.("ransomware-monitor", false);
        }
      }
    };
    void reconcile();
    return () => {
      cancelled = true;
      if (retryTimer) clearTimeout(retryTimer);
    };
  }, [enabled, hasPaid, threshold, windowSeconds, alertCooldownSeconds, attributionMinFiles, action, watchDirsJson, warnOnce, onStartupRearm]);
}
