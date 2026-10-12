// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, test } from 'bun:test';
import { firewallEnabled, parsePortSpec, providerLabel, requireFirewallReceipt, ruleKey, type PortGuardStatus } from '@/lib/portGuard';
import { PORT_PRESETS, PORT_PRESET_CATEGORIES } from '@/lib/portGuardPresets';

describe('passive Port Guard input and status', () => {
  test('firewall mutations require Windows read-back, not just a created message', () => {
    for (const value of [undefined, null, {}, { status: 'created' }, { verified: false }, { verified: 'true' }]) {
      let rejected = false; try { requireFirewallReceipt(value); } catch { rejected = true; }
      expect(rejected).toBe(true);
    }
    requireFirewallReceipt({ status: 'created', verified: true });
    requireFirewallReceipt({ status: 'already_absent', verified: true });
  });
  test('incoming presets exclude outgoing references without empty categories', () => {
    const preset = (name: string) => PORT_PRESETS.find(item => item.name === name);
    expect(preset('AnyDesk direct')).toMatchObject({ ports: '7070', protocol: 'tcp' });
    expect(preset('TeamViewer LAN')).toMatchObject({ ports: '5938', protocol: 'both' });
    for (const name of ['AnyDesk relay', 'TeamViewer service', 'Teams service', 'Zoom service TCP', 'Zoom service UDP', 'FTP active data source', 'PlayStation service', 'Epic-Games service', 'Battle.net service']) {
      expect(preset(name)).toBeUndefined();
    }
    for (const category of PORT_PRESET_CATEGORIES) expect(category.presets.length).toBeGreaterThan(0);
    expect(preset('NetBIOS datagram')?.protocol).toBe('udp');
    expect(preset('NetBIOS session')?.protocol).toBe('tcp');
    expect(preset('uTorrent')).toBeUndefined();
    expect(new Set(PORT_PRESETS.map(item => item.name)).size).toBe(PORT_PRESETS.length);
    for (const item of PORT_PRESETS) expect(parsePortSpec(item.ports).length).toBeGreaterThan(0);
  });
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
