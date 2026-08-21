import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AuthAnomalyTimeBasis } from "../types/settings";

export const DEFAULT_AUTH_FAILED_BURST_THRESHOLD = 5;
export const DEFAULT_AUTH_FAILED_BURST_WINDOW_SECS = 90;
export const DEFAULT_AUTH_WORK_START_HOUR = 7;
export const DEFAULT_AUTH_WORK_END_HOUR = 20;
export const DEFAULT_AUTH_WORK_DAYS = [1, 2, 3, 4, 5] as const;
export const DEFAULT_AUTH_ALERT_DEBOUNCE_SECS = 600;

export interface AuthAnomalyPolicy {
  failedBurstThreshold: number;
  failedBurstWindowSecs: number;
  workStartHour: number;
  workEndHour: number;
  workDays: number[];
  timeBasis: AuthAnomalyTimeBasis;
  detectRdp: boolean;
  detectNewAccounts: boolean;
  detectOffHours: boolean;
  alertDebounceSecs: number;
  reportToFleet: boolean;
}

/** Reconciles persisted access-session policy into the privileged sidecar. */
export default function useAuthAnomalyMonitor(
  enabled: boolean,
  policy: AuthAnomalyPolicy,
  isEntitled: boolean,
  fleetReportingRequired = false,
) {
  useEffect(() => {
    if (!isEntitled) return;
    let cancelled = false;
    let retryTimer: number | undefined;
    let retryAttempts = 0;
    const apply = async () => {
      try {
        if (!enabled) {
          // A stop is safety-critical: do not leave collection running merely
          // because synchronising its next-start policy failed.
          await invoke("stop_auth_anomaly_monitor");
        } else {
          // Apply policy before starting, so the first Security-log poll never
          // runs using a previous schedule or threshold.
          await invoke("set_auth_anomaly_config", {
            config: {
              ...policy,
              // Mirror the signed master policy immediately in the sidecar;
              // Free enforces this again at its IPC boundary.
              reportToFleet: fleetReportingRequired || policy.reportToFleet,
            },
          });
          if (!cancelled) await invoke("start_auth_anomaly_monitor");
        }
        if (!cancelled) {
          retryAttempts = 0;
          window.dispatchEvent(new CustomEvent("auth-anomaly-monitor-health", { detail: { error: null } }));
        }
      } catch (error) {
        console.warn(`[useAuthAnomalyMonitor] ${enabled ? "start" : "stop"} failed:`, error);
        if (!cancelled) {
          const message = `Access & Session Monitor is degraded: ${String(error)}`;
          window.dispatchEvent(new CustomEvent("auth-anomaly-monitor-health", { detail: { error: message } }));
          // Transient sidecar failures should self-heal without a settings
          // edit, but never create an unbounded background retry loop.
          if (retryAttempts < 3) {
            const delayMs = retryAttempts === 0 ? 5_000 : 10_000;
            retryAttempts += 1;
            retryTimer = window.setTimeout(() => void apply(), delayMs);
          }
        }
      }
    };
    void apply();
    return () => {
      cancelled = true;
      if (retryTimer !== undefined) window.clearTimeout(retryTimer);
    };
  }, [enabled, fleetReportingRequired, isEntitled, policy]);
}
