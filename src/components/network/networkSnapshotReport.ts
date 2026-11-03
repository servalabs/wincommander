import type { NetworkPortRow, NetworkPortsResult } from "../../hooks/useBackend";

export interface NetworkSnapshotFilters {
  protocol: string;
  state: string;
  direction: string;
  hideLoopback: boolean;
  showWindowsProcesses: boolean;
  search: string;
}

function cell(value: string | number | null): string {
  const text = String(value ?? "").replace(/[\r\n\t]+/g, " ");
  return /^\s*[=+\-@]/.test(text) ? `'${text}` : text;
}

/** Formats only the rows currently visible in the Active Ports view. */
export function formatNetworkSnapshotReport(
  snapshot: NetworkPortsResult,
  rows: NetworkPortRow[],
  capturedAt: Date,
  filters: NetworkSnapshotFilters,
): string {
  const header = [
    "WinCommander active ports snapshot",
    `Captured (UTC): ${capturedAt.toISOString()}`,
    "This is a point-in-time snapshot, not a traffic history or packet capture.",
    `Included rows: ${rows.length}; scanned rows: ${snapshot.rows.length}; scan capped: ${snapshot.truncated ? "yes" : "no"}.`,
    `Filters: protocol=${cell(filters.protocol)}; state=${cell(filters.state)}; direction=${cell(filters.direction)}; hide loopback=${filters.hideLoopback ? "yes" : "no"}; show Windows processes=${filters.showWindowsProcesses ? "yes" : "no"}; search=${cell(filters.search) || "(none)"}.`,
    "Rows reflect the current view filters; Windows may omit unavailable connection details.",
    "Protocol\tLocal address\tLocal port\tRemote address\tRemote port\tState\tProcess ID\tProcess",
  ];
  const entries = rows.map((row) => [
    row.proto, row.localAddr, row.localPort, row.remoteAddr, row.remotePort,
    row.state, row.pid, row.processName,
  ].map(cell).join("\t"));
  return [...header, ...entries].join("\n");
}
