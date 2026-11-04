import { describe, expect, test } from "bun:test";
import type { NetworkPortRow, NetworkPortsResult } from "../../hooks/useBackend";
import { formatSampledNetworkHistory, SampledNetworkHistory } from "./sampledNetworkHistory";

const row: NetworkPortRow = { proto: "TCP", localAddr: "192.0.2.10", localPort: 54000, remoteAddr: "198.51.100.1", remotePort: 443, state: "ESTABLISHED", pid: 7, processName: "browser", processPath: "C:\\private.exe" };
const snapshot = (rows: NetworkPortRow[], truncated = false): NetworkPortsResult => ({ status:"ok", durationMs:1, truncated, totals:{tcp:rows.length, udp:0, shown:rows.length}, rows });
const at = (seconds: number) => new Date(Date.UTC(2026, 0, 1, 0, 0, seconds));

describe("sampled network history", () => {
  test("is opt in, groups consecutive samples and separates a return after disappearance", () => {
    const h = new SampledNetworkHistory();
    h.capture(snapshot([row]), at(0));
    expect(h.count).toBe(0);
    h.start(at(1)); h.capture(snapshot([row, row]), at(2)); h.capture(snapshot([{...row, state:"CLOSE_WAIT"}]), at(12));
    expect(h.rows().map(record => [record.samples, record.state])).toEqual([[2, "CLOSE_WAIT"]]);
    h.capture(snapshot([]), at(22)); h.capture(snapshot([row]), at(32));
    expect(h.rows().map(record => record.samples)).toEqual([2,1]);
    h.stop(); h.capture(snapshot([row]), at(42));
    expect(h.scans).toBe(4);
  });
  test("marks gaps and capped or failed scans without joining uncertain observations", () => {
    const h = new SampledNetworkHistory(); h.start(at(0));
    h.capture(snapshot([row]), at(1)); h.failed(); h.capture(snapshot([row]), at(11));
    h.capture(snapshot([row], true), at(21)); h.capture(snapshot([row]), at(31));
    h.breakContinuity(); h.capture(snapshot([row]), at(41));
    expect(h.rows().map(record => record.samples)).toEqual([1,1,1,1,1]);
    expect([h.failedScans,h.cappedScans]).toEqual([1,1]);
    h.stop(); h.start(at(51)); h.capture(snapshot([row]), at(52));
    expect(h.periods).toBe(2); expect(h.count).toBe(6);
  });
  test("bounds retained data, omits paths, escapes cells and explains report limits", () => {
    const h = new SampledNetworkHistory(); h.start(at(0));
    h.capture(snapshot(Array.from({length:1001}, (_, pid) => ({...row, pid, processName: pid === 1000 ? "=formula\nextra" : "browser"}))), at(1));
    expect(h.count).toBe(1000); expect(h.evicted).toBe(1);
    const report = formatSampledNetworkHistory(h, at(2));
    expect(report).toContain("not a complete traffic log or packet capture");
    expect(report).toContain("failed scans: 0; capped scans: 0; older rows discarded: 1");
    expect(report).toContain("\t'=formula extra");
    expect(report).not.toContain("private.exe");
    expect(formatSampledNetworkHistory(h, at(2), true)).toContain("Recording: paused");
    h.clear(); expect(h.count).toBe(0); expect(h.running).toBe(false); expect(h.startedAt).toBeNull();
  });
});
