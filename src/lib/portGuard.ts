// SPDX-License-Identifier: AGPL-3.0-or-later
export type PortProtocol = 'tcp' | 'udp' | 'both';
export interface PortEntry { port: number; endPort: number; protocol: PortProtocol; label: string; enabled: boolean; custom: boolean }
export interface PortGuardStatus {
  schemaVersion: number; running: boolean; desiredEnabled: boolean; provider: string;
  health: string; coverage: string; coverageComplete: boolean; coverageNote: string;
  collectionSettingsRetained: boolean; collectorHealthy: boolean; lastError: string | null;
  observedAt: string; generation: number; observedEvents: number; droppedEvents: number; historyEvicted: number;
  historyError: string | null; historyRetention: string;
}
export interface PortGuardHit {
  id: number; port: number; protocol: string; service: string; peer: string; localAddress: string;
  outcome: string; loopback: boolean; detectedAt: string;
}
export interface FirewallBlock { Name: string; Protocol: string; Port: string; Direction: string; Enabled: boolean | number }
export function firewallEnabled(value: boolean | number): boolean { return value === true || value === 1; }
export function ruleKey(rule: Pick<PortEntry, 'port' | 'endPort' | 'protocol'>): string { return `${rule.protocol}:${rule.port}-${rule.endPort}`; }
export function portDisplay(rule: Pick<PortEntry, 'port' | 'endPort'>): string { return rule.port === rule.endPort ? `${rule.port}` : `${rule.port}–${rule.endPort}`; }
export function parsePortSpec(value: string): { port: number; endPort: number }[] {
  const parts = value.trim().split(',');
  if (!value.trim() || parts.length > 32) throw new Error('Enter 1–32 ports or ranges, separated by commas.');
  const result = parts.map(part => {
    const match = /^(\d+)(?:\s*-\s*(\d+))?$/.exec(part.trim());
    if (!match) throw new Error('Use ports or ranges such as 8080, 9000-9010.');
    const port = Number(match[1]); const endPort = Number(match[2] ?? match[1]);
    if (port < 1 || endPort > 65535 || port > endPort) throw new Error('Ports must be between 1 and 65535; range ends must follow their starts.');
    return { port, endPort };
  });
  const identities = result.map(range => `${range.port}-${range.endPort}`);
  if (new Set(identities).size !== identities.length) throw new Error('Remove duplicate ports or ranges.');
  return result;
}
export function providerLabel(status: PortGuardStatus | null, stale: boolean): string {
  if (stale) return 'Status unavailable';
  if (!status) return 'Checking';
  if (status.lastError) return 'Unavailable';
  if (!status.desiredEnabled) return 'Off';
  if (!status.running) return status.health === 'waiting-for-rules' ? 'Add a watch' : 'Starting';
  return status.coverageComplete && status.droppedEvents === 0 ? 'Monitoring' : 'Limited coverage';
}
