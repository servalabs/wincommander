import { describe, expect, test } from "bun:test";
import type { NetworkPortsResult } from "../../hooks/useBackend";
import { formatNetworkSnapshotReport } from "./networkSnapshotReport";

const filters = { protocol: "tcp", state: "all", direction: "all", hideLoopback: true,
  showWindowsProcesses: false, search: "example" };

describe("active ports snapshot report", () => {
  test("describes scope, cap and capture time and reports only supplied rows", () => {
    const row = { proto: "TCP" as const, localAddr: "127.0.0.1", localPort: 42,
      remoteAddr: "192.0.2.1", remotePort: 443, state: "ESTABLISHED", pid: 42,
      processName: "example\nprocess", processPath: "C:\\private.exe" };
    const snapshot: NetworkPortsResult = { status: "ok", durationMs: 20, truncated: true,
      totals: { tcp: 2, udp: 0, shown: 2 }, rows: [row, { ...row, pid: 99 }] };
    const report = formatNetworkSnapshotReport(snapshot, [row], new Date("2026-01-02T03:04:05Z"), filters);
    expect(report).toContain("2026-01-02T03:04:05.000Z");
    expect(report).toContain("not a traffic history or packet capture");
    expect(report).toContain("Included rows: 1; scanned rows: 2; scan capped: yes");
    expect(report).toContain("protocol=tcp; state=all; direction=all; hide loopback=yes; show Windows processes=no; search=example");
    expect(report).toContain("42\texample process");
    expect(report).not.toContain("private.exe");
    expect(report).not.toContain("\t99\t");
  });

  test("keeps process text inert when pasted into a spreadsheet", () => {
    const row = { proto: "TCP" as const, localAddr: "127.0.0.1", localPort: 1,
      remoteAddr: "", remotePort: 0, state: "LISTEN", pid: 1,
      processName: "=cmd()", processPath: null };
    const snapshot: NetworkPortsResult = { status: "ok", durationMs: 1, truncated: false,
      totals: { tcp: 1, udp: 0, shown: 1 }, rows: [row] };
    const report = formatNetworkSnapshotReport(snapshot, [row], new Date(0), { ...filters, search: "=cmd()\nforged" });
    expect(report).toContain("\t'=cmd()");
    expect(report).toContain("search='=cmd() forged");
  });
});
