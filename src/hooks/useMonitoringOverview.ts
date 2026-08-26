import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export type MonitorState = "active" | "alert" | "degraded" | "stale" | "idle" | "locked" | "unavailable";
export type MonitorHealth = "healthy" | "degraded" | "stale" | "locked" | "unavailable";

export interface MonitorSnapshot {
  id: string;
  label: string;
  group: string;
  requiresPro: boolean;
  running: boolean;
  state: MonitorState;
  health: MonitorHealth;
  recentCount: number | null;
  activeCount: number | null;
  lastActivityAt: string | null;
  startedAt: string | null;
  cadenceSecs: number | null;
  errorCode: string | null;
  capabilities: string[];
}

export interface MonitoringSummary {
  total: number;
  running: number;
  active: number;
  alerts: number;
  degraded: number;
  unavailable: number;
  locked: number;
}

export interface MonitoringOverview {
  schemaVersion: number;
  scope: string;
  observedAt: string;
  monitors: MonitorSnapshot[];
  summary: MonitoringSummary;
  pro: {
    state: "ready" | "locked" | "unavailable";
    available: boolean;
    errorCode: string | null;
  };
  privacy: {
    contentFree: boolean;
    rawEventsIncluded: boolean;
    identifiersIncluded: boolean;
  };
}

export interface MonitoringOverviewState {
  overview: MonitoringOverview | null;
  error: string | null;
  refresh: () => Promise<void>;
  loading: boolean;
  refreshing: boolean;
}

export function fetchMonitoringOverview(): Promise<MonitoringOverview> {
  return invoke<MonitoringOverview>("get_monitoring_overview");
}

/** Poll the single operations snapshot instead of waking every monitor card. */
export default function useMonitoringOverview(enabled = true): MonitoringOverviewState {
  const [overview, setOverview] = useState<MonitoringOverview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const inFlight = useRef(false);
  const mounted = useRef(true);

  const refresh = useCallback(async () => {
    if (!enabled || inFlight.current) return;
    inFlight.current = true;
    setRefreshing(true);
    try {
      const next = await fetchMonitoringOverview();
      if (mounted.current) {
        setOverview(next);
        setError(null);
      }
    } catch {
      // IPC errors can contain implementation details. Keep this panel's
      // content-free promise intact and leave diagnostics to the local log.
      if (mounted.current) setError("Monitor status could not be refreshed. Try again.");
    } finally {
      inFlight.current = false;
      if (mounted.current) setRefreshing(false);
    }
  }, [enabled]);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    if (!enabled) return;
    void refresh();
    const timer = window.setInterval(() => void refresh(), 15_000);
    return () => window.clearInterval(timer);
  }, [enabled, refresh]);

  return {
    overview,
    error,
    refresh,
    loading: refreshing && overview === null,
    refreshing,
  };
}
