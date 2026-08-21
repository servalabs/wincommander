// useWifiGuardMonitor — reconciles the Pro rogue-AP detector from persisted
// operator policy. Baseline identifiers stay on this device; Fleet reporting
// is handled separately by BackgroundPollers with a deliberately coarse event.

import { useEffect, useMemo, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { WifiGuardBaselineEntry } from "../types/settings";
import { showWarning } from "../utils/toast";

export const DEFAULT_WIFI_GUARD_LEARNING_WINDOW_SECS = 24 * 60 * 60;
export const DEFAULT_WIFI_GUARD_POLL_INTERVAL_SECS = 30;
export const DEFAULT_WIFI_GUARD_ALERT_DEBOUNCE_SECS = 300;

export interface WifiGuardPolicy {
  learningWindowSecs: number;
  learningUntil: string | null;
  pollIntervalSecs: number;
  alertDebounceSecs: number;
}

function stableBaseline(entries: WifiGuardBaselineEntry[]): WifiGuardBaselineEntry[] {
  return entries
    .map((entry) => ({
      ssid: entry.ssid,
      bssids: [...entry.bssids].sort(),
      bestAuthStrength: entry.bestAuthStrength,
    }))
    .sort((a, b) => a.ssid.localeCompare(b.ssid));
}

/**
 * Rehydrate the private sidecar from the public settings store, then start or
 * stop it according to the explicit arm switch. While armed, copy passive
 * learning back to settings every 30 seconds so a normal app restart does not
 * discard the trusted-network baseline.
 */
export default function useWifiGuardMonitor(
  enabled: boolean,
  policy: WifiGuardPolicy,
  baseline: WifiGuardBaselineEntry[],
  onRuntimeStateChanged: (next: WifiGuardBaselineEntry[], learningUntil: string | null) => void,
) {
  const baselineFingerprint = useMemo(
    () => JSON.stringify(stableBaseline(baseline)),
    [baseline],
  );
  // Use the fingerprint as the reconciliation dependency so harmless array
  // identity churn in settings does not reset the Pro detector.
  const baselineToApply = useMemo(
    () => JSON.parse(baselineFingerprint) as WifiGuardBaselineEntry[],
    [baselineFingerprint],
  );
  const appliedBaselineRef = useRef(baselineFingerprint);
  const callbackRef = useRef(onRuntimeStateChanged);
  const degradedWarningShownRef = useRef(false);
  callbackRef.current = onRuntimeStateChanged;

  useEffect(() => {
    let cancelled = false;
    const reconcile = async () => {
      // Stopping is deliberately independent of configuration: a bad saved
      // policy must never prevent the operator from disarming the detector.
      if (!enabled) {
        await invoke("stop_wifi_guard").catch((error) => {
          console.warn("[useWifiGuardMonitor] stop failed:", error);
        });
        return;
      }

      let configured = false;
      let started = false;
      let lastError: unknown = null;
      // The sidecar can still be coming up during desktop startup. Retry a
      // bounded number of times with short backoff, then surface one clear
      // warning instead of leaving a persisted “on” switch silently inert.
      for (const delayMs of [0, 1_500, 5_000]) {
        if (delayMs > 0) {
          await new Promise<void>((resolve) => window.setTimeout(resolve, delayMs));
        }
        if (cancelled) return;
        try {
          await invoke("configure_wifi_guard", {
            config: {
              learningWindowSecs: policy.learningWindowSecs,
              learningUntil: policy.learningUntil,
              pollIntervalSecs: policy.pollIntervalSecs,
              alertDebounceSecs: policy.alertDebounceSecs,
              baseline: baselineToApply,
            },
          });
          configured = true;
          appliedBaselineRef.current = baselineFingerprint;
        } catch (error) {
          lastError = error;
          console.warn("[useWifiGuardMonitor] policy sync failed:", error);
        }
        try {
          // Still attempt a start if policy sync failed: the sidecar's safe
          // defaults are better than silently dropping requested coverage.
          await invoke("start_wifi_guard");
          started = true;
        } catch (error) {
          lastError = error;
          console.warn("[useWifiGuardMonitor] start failed:", error);
        }
        if (configured && started) {
          degradedWarningShownRef.current = false;
          return;
        }
      }
      if (!cancelled && !degradedWarningShownRef.current) {
        degradedWarningShownRef.current = true;
        showWarning(
          "Wi-Fi Guard could not apply its saved policy. Check WinCommander Pro, then open Network → Wi-Fi Guard to retry.",
          12_000,
        );
        console.warn("[useWifiGuardMonitor] detector remains degraded:", lastError);
      }
    };
    void reconcile();
    return () => { cancelled = true; };
  }, [enabled, policy.learningWindowSecs, policy.learningUntil, policy.pollIntervalSecs, policy.alertDebounceSecs, baselineFingerprint, baselineToApply]);

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    const syncBaseline = async () => {
      try {
        const [learned, status] = await Promise.all([
          invoke<WifiGuardBaselineEntry[]>("get_wifi_guard_baseline"),
          invoke<{ learningUntil?: string | null }>("wifi_guard_status"),
        ]);
        if (cancelled) return;
        const fingerprint = JSON.stringify(stableBaseline(learned));
        const learningUntil = status.learningUntil ?? null;
        if (fingerprint === appliedBaselineRef.current && learningUntil === policy.learningUntil) return;
        appliedBaselineRef.current = fingerprint;
        callbackRef.current(stableBaseline(learned), learningUntil);
      } catch {
        // Best effort: do not interrupt local detection if settings persistence
        // is temporarily unavailable.
      }
    };
    void syncBaseline();
    const timer = window.setInterval(() => void syncBaseline(), 30_000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [enabled, baselineFingerprint, policy.learningUntil]);
}
