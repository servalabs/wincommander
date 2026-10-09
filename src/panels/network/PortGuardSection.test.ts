// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, test } from 'bun:test';
import { firewallEnabled, parsePortSpec, providerLabel, ruleKey, type PortGuardStatus } from '@/lib/portGuard';

describe('passive Port Guard input and status', () => {
  test('Windows enum False=2 stays disabled', () => {
    expect(firewallEnabled(2)).toBe(false); expect(firewallEnabled(false)).toBe(false);
    expect(firewallEnabled(1)).toBe(true); expect(firewallEnabled(true)).toBe(true);
  });
  test('preserves ranges and lists instead of silently discarding preset ports', () => {
    expect(parsePortSpec('137, 138, 139, 8000-8010')).toEqual([
      { port: 137, endPort: 137 }, { port: 138, endPort: 138 },
      { port: 139, endPort: 139 }, { port: 8000, endPort: 8010 },
    ]);
  });
  test('rejects port wraparound, reversed ranges and partial numeric input', () => {
    for (const input of ['0', '65536', '-1', '22.5', '80-79', '22oops', '22,22', '']) {
      let rejected = false; try { parsePortSpec(input); } catch { rejected = true; }
      expect(rejected).toBe(true);
    }
  });
  test('TCP and UDP rules on the same port retain independent identities', () => {
    expect(ruleKey({ port: 53, endPort: 53, protocol: 'tcp' })).not.toBe(ruleKey({ port: 53, endPort: 53, protocol: 'udp' }));
  });
  test('saved enabled configuration never represents provider failure as healthy', () => {
    const status = { desiredEnabled: true, running: false, lastError: 'Access denied', health: 'unavailable' } as PortGuardStatus;
    expect(providerLabel(status, false)).toBe('Unavailable');
    expect(providerLabel({ ...status, running: true, lastError: null, coverageComplete: false }, false)).toBe('Limited coverage');
    expect(providerLabel({ ...status, running: true, lastError: null }, true)).toBe('Status unavailable');
  });
});
