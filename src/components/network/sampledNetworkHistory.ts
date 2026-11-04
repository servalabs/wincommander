import type { NetworkPortRow, NetworkPortsResult } from "../../hooks/useBackend";

export interface SampledEndpoint {
  firstSeen: string;
  lastSeen: string;
  samples: number;
  proto: string;
  localAddr: string;
  localPort: number;
  remoteAddr: string;
  remotePort: number;
  state: string;
  pid: number;
  processName: string | null;
}

const CAP = 1000;
const keyOf = (row: NetworkPortRow) => JSON.stringify([row.proto, row.localAddr, row.localPort, row.remoteAddr, row.remotePort, row.pid, row.processName]);
const cell = (value: string | number | null) => {
  const text = String(value ?? "").replace(/[\r\n\t]+/g, " ");
  return /^\s*[=+\-@]/.test(text) ? `'${text}` : text;
};

/** In-memory observations from periodic snapshots; no traffic or OS settings are changed. */
export class SampledNetworkHistory {
  running = false;
  generation = 0;
  startedAt: string | null = null;
  lastSampleAt: string | null = null;
  scans = 0;
  failedScans = 0;
  cappedScans = 0;
  evicted = 0;
  periods = 0;
  private nextId = 0;
  private active = new Map<string, number>();
  private records = new Map<number, SampledEndpoint>();

  start(at: Date): void {
    if (this.running) return;
    this.running = true;
    this.generation++;
    this.startedAt ??= at.toISOString();
    this.periods++;
    this.active.clear();
  }
  stop(): void { this.running = false; this.generation++; this.active.clear(); }
  breakContinuity(): void { this.generation++; this.active.clear(); }
  failed(): void { if (this.running) { this.failedScans++; this.active.clear(); } }
  clear(): void {
    this.generation++;
    this.running = false; this.startedAt = null; this.lastSampleAt = null;
    this.scans = 0; this.failedScans = 0; this.cappedScans = 0; this.evicted = 0; this.periods = 0;
    this.active.clear(); this.records.clear(); this.nextId = 0;
  }
  get count(): number { return this.records.size; }
  rows(): SampledEndpoint[] { return [...this.records.values()].map(row => ({ ...row })); }

  capture(snapshot: NetworkPortsResult, at: Date): void {
    if (!this.running) return;
    const time = at.toISOString();
    this.scans++; this.lastSampleAt = time;
    if (snapshot.truncated) { this.cappedScans++; this.active.clear(); }
    const seen = new Set<string>();
    for (const row of snapshot.rows) {
      const key = keyOf(row);
      if (seen.has(key)) continue;
      seen.add(key);
      const id = this.active.get(key);
      const existing = id === undefined ? undefined : this.records.get(id);
      if (existing) { existing.lastSeen = time; existing.samples++; existing.state = row.state; continue; }
      const record: SampledEndpoint = { firstSeen: time, lastSeen: time, samples: 1, proto: row.proto,
        localAddr: row.localAddr, localPort: row.localPort, remoteAddr: row.remoteAddr, remotePort: row.remotePort,
        state: row.state, pid: row.pid, processName: row.processName };
      const newId = this.nextId++;
      this.records.set(newId, record); this.active.set(key, newId);
      while (this.records.size > CAP) {
        const oldest = this.records.keys().next().value as number;
        const removed = this.records.get(oldest)!;
        this.records.delete(oldest); this.evicted++;
        const removedKey = JSON.stringify([removed.proto, removed.localAddr, removed.localPort, removed.remoteAddr, removed.remotePort, removed.pid, removed.processName]);
        if (this.active.get(removedKey) === oldest) this.active.delete(removedKey);
      }
    }
    for (const key of this.active.keys()) if (!seen.has(key)) this.active.delete(key);
    if (snapshot.truncated) this.active.clear();
  }
}

export function formatSampledNetworkHistory(history: SampledNetworkHistory, at: Date, paused = false): string {
  const header = [
    "WinCommander sampled network endpoint report",
    `Exported (UTC): ${at.toISOString()}; first recording started (UTC): ${history.startedAt ?? "unknown"}.`,
    "This is a sample of endpoints visible during refreshes, not a complete traffic log or packet capture.",
    "Connections between scans may be missed; entries can represent listeners or UDP endpoints, not actual transfers. Direction and connection start/end are not established.",
    "The same endpoint may be reused, and a gap or capped scan can split observations. Samples are not counts of connections.",
    `Recording: ${history.running ? paused ? "paused" : "on" : "off"}; recording periods: ${history.periods}; successful scans: ${history.scans}; failed scans: ${history.failedScans}; capped scans: ${history.cappedScans}; older rows discarded: ${history.evicted}.`,
    `Last successful scan (UTC): ${history.lastSampleAt ?? "none"}; retained rows: ${history.count}; retention limit: ${CAP}.`,
    "All rows returned by Windows were sampled, regardless of the view filters. Recording stops when this view closes; data is held in memory only.",
    "First seen (UTC)\tLast seen (UTC)\tSamples\tProtocol\tLocal address\tLocal port\tRemote address\tRemote port\tLast state\tProcess ID\tProcess",
  ];
  return [...header, ...history.rows().map(row => [row.firstSeen, row.lastSeen, row.samples, row.proto, row.localAddr, row.localPort, row.remoteAddr, row.remotePort, row.state, row.pid, row.processName].map(cell).join("\t"))].join("\n");
}
