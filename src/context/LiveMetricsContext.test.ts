import { describe, expect, test } from "bun:test";
import { mapLiveMetrics } from "./LiveMetricsContext";

describe("live metrics mapping", () => {
  test("keeps volatile samples in the dedicated dashboard shape", () => {
    const metrics = mapLiveMetrics({
      cpuUsage: 42.6,
      cpuTemp: 71.8,
      ramUsagePercent: 58.4,
      ramUsedGb: 9.876,
      ramTotalGb: 15.987,
      disks: [
        { name: "C:", totalGb: 100.04, freeGb: 25.01 },
        { name: "Docker", totalGb: 0, freeGb: 0 },
      ],
    });

    expect(metrics).toEqual({
      cpuUsage: 43,
      cpuTemp: 72,
      cpuTempStatus: 'live', cpuTempAgeMs: null,
      disksStatus: 'live', disksAgeMs: null,
      ramUsage: 58,
      ramUsedGb: 9.9,
      ramTotalGb: 16,
      disks: [{
        id: "C:", label: "C:", totalGb: 100, usedGb: 75, freeGb: 25,
        percent: 75,
      }],
    });
  });

  test("does not invent a temperature when the host cannot report one", () => {
    const metrics = mapLiveMetrics({
      cpuUsage: 1,
      cpuTemp: 0,
      ramUsagePercent: 2,
      ramUsedGb: 1,
      ramTotalGb: 2,
      disks: [],
    });

    expect(metrics.cpuTemp).toBeNull();
    expect(metrics.cpuTempStatus).toBe('unavailable');
  });

  test('retains last observed values without labelling delayed probes as live', () => {
    const metrics = mapLiveMetrics({
      cpuUsage: 1, cpuTemp: 55, ramUsagePercent: 2, ramUsedGb: 1, ramTotalGb: 2,
      disks: [{ name: 'C:', totalGb: 100, freeGb: 50 }],
      cpuTempStatus: 'stale', cpuTempAgeMs: 60000, disksStatus: 'stale', disksAgeMs: 12000,
    });
    expect(metrics.cpuTemp).toBe(55);
    expect(metrics.cpuTempStatus).toBe('stale');
    expect(metrics.cpuTempAgeMs).toBe(60000);
    expect(metrics.disksStatus).toBe('stale');
    expect(metrics.disksAgeMs).toBe(12000);
    expect(metrics.disks).toHaveLength(1);
  });
});
