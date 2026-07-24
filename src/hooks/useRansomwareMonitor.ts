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

import { useEffect, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { RansomwareAction } from "../types/settings";

export const DEFAULT_RANSOMWARE_THRESHOLD = 50;
export const DEFAULT_RANSOMWARE_WINDOW_SECONDS = 30;
// Suspend-by-default per roadmap (F-3 v2): reversible, names the culprit.
export const DEFAULT_RANSOMWARE_ACTION: RansomwareAction = "suspend";

export default function useRansomwareMonitor(
  enabled: boolean,
  threshold: number,
  windowSeconds: number,
  customWatchDirs: string[],
  action: RansomwareAction,
) {
  const watchDirsFingerprint = useMemo(
    () => customWatchDirs.join("\n"),
    [customWatchDirs],
  );

  // Push config first — if the watcher is already running, the next
  // event will use these bounds; if it's about to start, the config
  // is in place by the time it spins up. `action` only affects the Pro
  // ETW path (the notify fallback can't attribute a PID), but we always
  // forward it so a live ETW session picks up a preset change at once.
  useEffect(() => {
    invoke("set_ransomware_config", {
      config: { threshold, windowSeconds, action },
    }).catch((err) => {
      console.warn("[useRansomwareMonitor] set_config failed:", err);
    });
  }, [threshold, windowSeconds, action]);

  // Sync custom watch dirs. Rust diff-reconciles against the running
  // watcher, so this is safe to call whenever the array changes.
  useEffect(() => {
    invoke("set_ransomware_watch_dirs", { dirs: customWatchDirs }).catch((err) => {
      console.warn("[useRansomwareMonitor] set_watch_dirs failed:", err);
    });
    // Use a stable fingerprint to avoid spurious effect re-runs from
    // array-identity churn during patchAppSettings.
  }, [customWatchDirs, watchDirsFingerprint]);

  // Start / stop.
  useEffect(() => {
    if (enabled) {
      invoke("start_ransomware_monitor").catch((err) => {
        console.warn("[useRansomwareMonitor] start failed:", err);
      });
    } else {
      invoke("stop_ransomware_monitor").catch((err) => {
        console.warn("[useRansomwareMonitor] stop failed:", err);
      });
    }
  }, [enabled]);
}
