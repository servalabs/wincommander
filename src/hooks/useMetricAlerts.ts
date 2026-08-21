// src/hooks/useMetricAlerts.ts
//
// Reusable per-metric alert config (CPU / RAM / Upload / Download). One shared store
// so every card that surfaces a metric alert (NetworkTrafficCard, the merged
// CPU/Memory card, …) reads and writes the SAME config and stays in sync.
//
// Backend authority: net_traffic_alert.rs (metric_alerts_get_config /
// metric_alerts_set_config). Adding a new metric is: add a key here + a field
// in the Rust config + sample it in the sampler.

import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export interface MetricAlert {
  enabled: boolean;
  /** Limit in the metric's own unit (CPU = %, upload/download = MB/s). */
  threshold: number;
  hysteresisEnabled: boolean;
  hysteresisPct: number;
  sustainedEnabled: boolean;
  sustainedSecs: number;
  /** Forward this alert to the Fleet console when it fires. Settings path
   *  `notifications.{cpuUsage,ramUsage,networkUsage}.reportToFleet` — see
   *  PrivacyShieldSettings.fleetManaged for the equivalent admin-lock pattern. */
  reportToFleet: boolean;
}

export interface MetricAlertsConfig {
  cpu: MetricAlert;
  ram: MetricAlert;
  upload: MetricAlert;
  download: MetricAlert;
}

export type MetricKey = keyof MetricAlertsConfig;

// ── Module-level shared store ────────────────────────────────────────
let _cache: MetricAlertsConfig | null = null;
let _loading = false;
const _subs = new Set<(c: MetricAlertsConfig | null) => void>();

function notify() {
  for (const fn of _subs) fn(_cache);
}

async function ensureLoaded() {
  if (_cache || _loading) return;
  _loading = true;
  try {
    _cache = await invoke<MetricAlertsConfig>("metric_alerts_get_config");
    notify();
  } catch {
    // best-effort — leave null; cards render nothing until it loads
  } finally {
    _loading = false;
  }
}

export function useMetricAlerts() {
  const [config, setConfig] = useState<MetricAlertsConfig | null>(_cache);

  useEffect(() => {
    const fn = (c: MetricAlertsConfig | null) => setConfig(c);
    _subs.add(fn);
    void ensureLoaded();
    return () => { _subs.delete(fn); };
  }, []);

  // Patch one metric and persist the whole config. Best-practice suppressors
  // (hysteresis + sustained) are locked on so a value hovering near the limit
  // doesn't spam notifications — only enable + threshold are user-facing.
  const update = useCallback(async (metric: MetricKey, patch: Partial<MetricAlert>) => {
    const base = _cache ?? (await invoke<MetricAlertsConfig>("metric_alerts_get_config"));
    const merged: MetricAlert = {
      ...base[metric],
      ...patch,
      hysteresisEnabled: true,
      hysteresisPct: (patch.hysteresisPct ?? base[metric].hysteresisPct) || 20,
      sustainedEnabled: true,
      // Respect a patched hold time; fall back to the existing value, then 30s.
      sustainedSecs: (patch.sustainedSecs ?? base[metric].sustainedSecs) || 30,
    };
    const next: MetricAlertsConfig = { ...base, [metric]: merged };
    const saved = await invoke<MetricAlertsConfig>("metric_alerts_set_config", { config: next });
    _cache = saved;
    notify();
    return saved;
  }, []);

  return { config, update };
}
