import { createContext, useCallback, useContext, useMemo, useRef, useState, type ReactNode } from "react";
import useBackend, { type LiveMetricsResult, type SystemInfo } from "../hooks/useBackend";

type LiveMetricDisk = SystemInfo["disks"][number];

export interface LiveMetrics {
  cpuUsage: number;
  cpuTemp: number | null;
  ramUsage: number;
  ramUsedGb: number;
  ramTotalGb: number;
  disks: LiveMetricDisk[];
}

export type LiveMetricsStatus = "loading" | "live" | "stale";

interface LiveMetricsState {
  metrics: LiveMetrics | null;
  status: LiveMetricsStatus;
  refreshLiveMetrics: () => Promise<void>;
}

const LiveMetricsContext = createContext<LiveMetricsState | null>(null);

function round1(value: number): number {
  return Math.round(value * 10) / 10;
}

function mapLiveMetrics(raw: LiveMetricsResult): LiveMetrics {
  return {
    cpuUsage: Math.round(raw.cpuUsage),
    cpuTemp: raw.cpuTemp != null && raw.cpuTemp > 0 ? Math.round(raw.cpuTemp) : null,
    ramUsage: Math.round(raw.ramUsagePercent),
    ramUsedGb: round1(raw.ramUsedGb),
    ramTotalGb: round1(raw.ramTotalGb),
    disks: raw.disks
      .filter((disk) => disk.totalGb > 0.1)
      .map((disk) => {
        const totalGb = round1(disk.totalGb);
        const freeGb = round1(disk.freeGb);
        const usedGb = round1(totalGb - freeGb);
        return {
          id: disk.name,
          label: disk.name,
          totalGb,
          usedGb,
          freeGb,
          percent: totalGb > 0 ? Math.round((usedGb / totalGb) * 100) : 0,
        };
      }),
  };
}

/**
 * Live CPU/RAM/disk samples have their own context because they change every
 * two seconds. Keeping them out of AppContext prevents a dashboard gauge tick
 * from invalidating settings forms, navigation, and hidden panels.
 */
export function LiveMetricsProvider({ children }: { children: ReactNode }) {
  const [metrics, setMetrics] = useState<LiveMetrics | null>(null);
  const [status, setStatus] = useState<LiveMetricsStatus>("loading");
  const { getLiveMetrics } = useBackend();
  const inFlightRef = useRef<Promise<void> | null>(null);

  const refreshLiveMetrics = useCallback(async () => {
    if (inFlightRef.current) return inFlightRef.current;
    const request = (async () => {
      try {
        const raw = await getLiveMetrics();
        setMetrics(mapLiveMetrics(raw));
        setStatus("live");
      } catch {
        // Preserve the last successful reading. The dashboard can continue to
        // show it as stale when a transient sysinfo IPC call fails.
        setStatus("stale");
      }
    })();
    inFlightRef.current = request;
    try {
      await request;
    } finally {
      if (inFlightRef.current === request) inFlightRef.current = null;
    }
  }, [getLiveMetrics]);

  const value = useMemo(
    () => ({ metrics, status, refreshLiveMetrics }),
    [metrics, status, refreshLiveMetrics],
  );

  return <LiveMetricsContext.Provider value={value}>{children}</LiveMetricsContext.Provider>;
}

export function useLiveMetrics(): LiveMetricsState {
  const context = useContext(LiveMetricsContext);
  if (!context) {
    throw new Error("useLiveMetrics must be used within a LiveMetricsProvider");
  }
  return context;
}

export { mapLiveMetrics };
