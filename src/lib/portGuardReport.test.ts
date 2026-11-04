import { describe, expect, test } from 'bun:test';
import type { PortGuardHit, PortGuardStatus } from './portGuard';
import { formatPortGuardReport } from './portGuardReport';

const hit: PortGuardHit = { id: 1, port: 22, protocol: 'tcp', service: 'SSH', peer: '192.0.2.2:10', localAddress: '192.0.2.1', outcome: 'blocked', loopback: false, detectedAt: '2026-01-02T01:00:00Z' };
const status: PortGuardStatus = { schemaVersion: 2, running: true, desiredEnabled: true, provider: 'test', health: 'ok', coverage: 'limited', coverageComplete: false, coverageNote: 'Not every packet is seen', collectionSettingsRetained: true, collectorHealthy: true, lastError: null, observedAt: '2026-01-02T00:00:00Z', generation: 2, observedEvents: 9, droppedEvents: 3, historyEvicted: 4, historyError: null, historyRetention: 'bounded' };

describe('Port Guard history report', () => {
  test('reports raw retained rows and the limits of the observed history', () => {
    const report = formatPortGuardReport([hit, { ...hit, id: 2 }], status, true, 'read unavailable', new Date('2026-01-03T00:00:00Z'));
    expect(report).toContain('not a complete network history or packet capture');
    expect(report).toContain('Retained rows: 2; status: stale');
    expect(report).toContain('queue losses: 3; older rows expired: 4');
    expect(report).toContain('read issue: read unavailable');
    expect(report.split('2026-01-02T01:00:00Z\tSSH').length - 1).toBe(2);
  });
  test('handles unavailable status and makes untrusted cells safe to paste', () => {
    const report = formatPortGuardReport([{ ...hit, service: '=cmd()\nextra', peer: '@example' }], null, true, null, new Date(0));
    expect(report).toContain('watching: unknown');
    expect(report).toContain("\t'=cmd() extra\t22\ttcp\t'@example\t");
  });
});
