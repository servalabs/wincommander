// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, test } from 'bun:test';
import type { PortGuardHit } from './portGuard';
import { groupPortGuardObservations } from './portGuardHistory';

function hit(id: number, seconds: number, patch: Partial<PortGuardHit> = {}): PortGuardHit {
  return { id, port: 3306, protocol: 'tcp', service: 'MySQL', peer: '192.0.2.2:55000',
    localAddress: '192.0.2.1', outcome: 'blocked', loopback: false,
    detectedAt: new Date(Date.UTC(2026, 9, 9) + seconds * 1000).toISOString(), ...patch };
}
describe('Port Guard observation groups', () => {
  test('groups repeated endpoint observations without deleting evidence or claiming sessions', () => {
    const rows = [hit(1, 0), hit(2, 1), hit(3, 3), hit(4, 7)];
    const snapshot = JSON.stringify(rows);
    const groups = groupPortGuardObservations(rows);
    expect(groups.length).toBe(1);
    expect(groups[0].latest.id).toBe(4);
    expect(groups[0].observations.map(row => row.id)).toEqual([4, 3, 2, 1]);
    expect(JSON.stringify(rows)).toBe(snapshot);
  });
  test('separates endpoints, protocol, outcome, local address, watch label and loopback', () => {
    for (const patch of [{ port: 7070 }, { peer: '192.0.2.2:55001' }, { protocol: 'udp' },
      { outcome: 'allowed' }, { localAddress: '192.0.2.3' }, { service: 'Other' }, { loopback: true }]) {
      expect(groupPortGuardObservations([hit(1, 0), hit(2, 1, patch)]).length).toBe(2);
    }
  });
  test('caps each group at thirty seconds instead of chaining ongoing activity forever', () => {
    expect(groupPortGuardObservations([hit(1, 0), hit(2, 20), hit(3, 40)]).map(group => group.observations.length)).toEqual([2, 1]);
    expect(groupPortGuardObservations([hit(1, 0), hit(2, 30)]).length).toBe(1);
    expect(groupPortGuardObservations([hit(1, 0), hit(2, 30.001)]).length).toBe(2);
  });
  test('does not guess a time window for malformed timestamps', () => {
    expect(groupPortGuardObservations([hit(1, 0, { detectedAt: 'invalid' }), hit(2, 0, { detectedAt: 'invalid' })]).length).toBe(2);
    expect(groupPortGuardObservations([])).toEqual([]);
  });
});
