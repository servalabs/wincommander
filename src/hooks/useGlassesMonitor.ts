// src/hooks/useGlassesMonitor.ts
//
// useGlassesMonitor — drives the Free smart-glasses BLE monitor off
// settings. Same shape as useRemoteAccessMonitor: one global hook reads
// `enabled` and reconciles the Rust runtime (start is idempotent).

import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { StartupProtectionOperation } from "../lib/startupProtectionReadiness";
import { newDiagnosticOperationId, recordDiagnostic } from "../lib/diagnostics";

export interface GlassesDetection {
  vendor: string;
  rule: string;
  confidence: "low" | "medium" | "high";
  rssi: number;
  detectedAt: string;
}

export interface GlassesMonitorStatus {
  running: boolean;
  radio: "stopped" | "starting" | "scanning" | "radio_unavailable" | "error";
  recentCount: number;
}

export const getGlassesMonitorStatus = () => invoke<GlassesMonitorStatus>("glasses_monitor_status");
export const getGlassesMonitorRecent = () => invoke<GlassesDetection[]>("get_glasses_monitor_recent");
export const clearGlassesMonitorRecent = () => invoke("clear_glasses_monitor_recent");

export default function useGlassesMonitor(
  enabled: boolean,
  onStartupRearm?: (operation: StartupProtectionOperation, succeeded: boolean) => void,
) {
  const lastReconciled = useRef<boolean | null>(null);

  useEffect(() => {
    // Skip only a previously successful reconciliation so a startup race
    // cannot leave an ON preference silently inert.
    if (lastReconciled.current === enabled) return;
    let cancelled = false;
    let retryTimer: ReturnType<typeof setTimeout> | null = null;
    let attempt = 0;
    const operationId = newDiagnosticOperationId("glasses");
    const reconcile = async () => {
      if (cancelled) return;
      try {
        await invoke(enabled ? "start_glasses_monitor" : "stop_glasses_monitor");
        if (cancelled) return;
        lastReconciled.current = enabled;
        if (enabled) onStartupRearm?.("glasses-monitor", true);
      } catch (err) {
        console.warn("[useGlassesMonitor] reconcile failed:", err);
        recordDiagnostic({ operationId, feature: "glasses", action: enabled ? "start" : "stop", stage: "runtime",
          lifecycle: "applied", outcome: "failed", errorCode: enabled ? "GLASSES.START_FAILED" : "GLASSES.STOP_FAILED",
          severity: "warn", retryability: "automatic", suggestedNextAction: "retry", privacyClass: "local_sensitive",
          context: { state: enabled ? "enabled" : "disabled", retry_count: attempt } });
        attempt += 1;
        if (!cancelled && attempt < 3) {
          retryTimer = setTimeout(() => { void reconcile(); }, attempt * 5_000);
        } else if (!cancelled && enabled) {
          onStartupRearm?.("glasses-monitor", false);
        }
      }
    };
    void reconcile();
    return () => {
      cancelled = true;
      if (retryTimer) clearTimeout(retryTimer);
    };
  }, [enabled, onStartupRearm]);
}
