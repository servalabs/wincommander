import { useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { AppSettings, WifiGuardBaselineEntry } from '../types/settings';
import type { StartupProtectionOperation } from '../lib/startupProtectionReadiness';
import { recordDiagnostic } from '../lib/diagnostics';
import { isWifiGuardRecoveryBlocked, withWifiGuardOperation, type WifiGuardStatus } from '../lib/wifiGuard';

export const DEFAULT_WIFI_GUARD_LEARNING_WINDOW_SECS = 24 * 60 * 60;
export const DEFAULT_WIFI_GUARD_POLL_INTERVAL_SECS = 10;
export const DEFAULT_WIFI_GUARD_ALERT_DEBOUNCE_SECS = 300;

export interface WifiGuardPolicy {
  learningWindowSecs: number;
  learningUntil: string | null;
  pollIntervalSecs: number;
  alertDebounceSecs: number;
}

export default function useWifiGuardMonitor(
  enabled: boolean,
  policy: WifiGuardPolicy,
  baseline: WifiGuardBaselineEntry[],
  _onRuntimeStateChanged: (next: WifiGuardBaselineEntry[], learningUntil: string | null) => Promise<void>,
  onStartupRearm?: (operation: StartupProtectionOperation, succeeded: boolean) => void,
  available = true,
) {
  const desired = JSON.stringify({
    learningWindowSecs: policy.learningWindowSecs, learningUntil: null,
    pollIntervalSecs: policy.pollIntervalSecs, alertDebounceSecs: policy.alertDebounceSecs, baseline,
  });
  const callback = useRef(onStartupRearm);
  callback.current = onStartupRearm;
  const previousAvailability = useRef(available);
  const cleanupNeeded = useRef(false);

  useEffect(() => {
    if (previousAvailability.current && !available) cleanupNeeded.current = true;
    previousAvailability.current = available;
    // A Free-only startup must not create/retry paid sidecars. Expiry cleanup
    // can still use the narrowly allowed stop command once it is needed.
    if (!available && !cleanupNeeded.current) return;
    let cancelled = false;
    let pending = false;
    let appliedInstance: string | null = null;
    let appliedPolicy: string | null = null;
    let reportedHealth: boolean | null = null;
    let stopConfirmed = false;
    const reconcile = async () => {
      if (cancelled || pending || ((!enabled || !available) && stopConfirmed)) return;
      pending = true;
      try {
        await withWifiGuardOperation(async () => {
          if (cancelled) return;
          if (!available) {
            await invoke('stop_wifi_guard');
            cleanupNeeded.current = false;
            stopConfirmed = true;
            return;
          }
          const authoritative = await invoke<AppSettings>('get_settings');
          if (cancelled) return;
          const saved = authoritative.ideal?.network?.wifiGuard;
          if (!saved?.enabled) {
            await invoke('stop_wifi_guard');
            const stopped = await invoke<WifiGuardStatus>('wifi_guard_status');
            if (stopped.running) throw new Error('Wi-Fi observer stop is not confirmed');
            stopConfirmed = true;
            return;
          }
          if (isWifiGuardRecoveryBlocked()) return;
          const confirmedPolicy = JSON.stringify({
            learningWindowSecs: saved.learningWindowSecs ?? DEFAULT_WIFI_GUARD_LEARNING_WINDOW_SECS,
            learningUntil: null,
            pollIntervalSecs: saved.pollIntervalSecs ?? DEFAULT_WIFI_GUARD_POLL_INTERVAL_SECS,
            alertDebounceSecs: saved.alertDebounceSecs ?? DEFAULT_WIFI_GUARD_ALERT_DEBOUNCE_SECS,
            baseline: saved.baseline ?? [],
          });
          let status = await invoke<WifiGuardStatus>('wifi_guard_status');
          if (cancelled) return;
          if (status.schemaVersion !== 2) throw new Error('Wi-Fi Guard update required');
          if (appliedInstance !== status.instanceId || appliedPolicy !== confirmedPolicy) {
            status = await invoke<WifiGuardStatus>('configure_wifi_guard', { config: JSON.parse(confirmedPolicy) });
            if (cancelled) return;
            appliedInstance = status.instanceId;
            appliedPolicy = confirmedPolicy;
          }
          if (!status.running) {
            await invoke('start_wifi_guard');
            if (cancelled) return;
            status = await invoke<WifiGuardStatus>('wifi_guard_status');
          }
          if (cancelled) return;
          const healthy = status.running && status.collectorHealthy;
          if (reportedHealth !== healthy) {
            reportedHealth = healthy;
            const onStartupRearm = callback.current;
            if (healthy) onStartupRearm?.("wifi-guard", true);
            else onStartupRearm?.('wifi-guard', false);
          }
        });
      } catch {
        if (!cancelled) {
          callback.current?.('wifi-guard', false);
          if (reportedHealth !== false) recordDiagnostic({ feature: 'wifi_guard', action: 'start', stage: 'sidecar', lifecycle: 'applied', outcome: 'failed', severity: 'warn', retryability: 'automatic', suggestedNextAction: 'retry', privacyClass: 'restricted', errorCode: 'WIFI.GUARD.START_FAILED' });
          reportedHealth = false;
        }
      } finally { pending = false; }
    };
    void reconcile();
    const timer = window.setInterval(() => void reconcile(), 15_000);
    return () => { cancelled = true; window.clearInterval(timer); };
  }, [enabled, desired, available]);
}
