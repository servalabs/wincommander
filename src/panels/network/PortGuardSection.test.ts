// src/panels/network/PortGuardSection.test.ts
//
// Regression coverage for mergeEntries()'s port/firewall-rule merge
// logic. See PortGuardSection.tsx for context — two independent
// firewall rules on the same single numeric port (e.g. a standalone
// Inbound block plus a later Outbound block) must both remain visible
// and independently removable, not silently collapse into one row.

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { mergeEntries, type FirewallBlock, type PortEntry } from './PortGuardSection';

function fw(overrides: Partial<FirewallBlock>): FirewallBlock {
  return {
    Name: 'rule',
    Protocol: 'TCP',
    Port: '3389',
    Direction: 'Inbound',
    Enabled: true,
    ...overrides,
  };
}

describe('mergeEntries', () => {
  test('two independent firewall rules on the same port both stay visible and removable', () => {
    const blocks: FirewallBlock[] = [
      fw({ Name: 'RDP-In', Port: '3389', Direction: 'Inbound' }),
      fw({ Name: 'RDP-Out', Port: '3389', Direction: 'Outbound' }),
    ];

    const rows = mergeEntries([], blocks, false, new Set(), new Set());

    // Both rules must be present as their own rows — neither should
    // have been overwritten/dropped by the other.
    const names = rows.filter((r) => r.firewall).map((r) => r.firewallName);
    expect(names).toContain('RDP-In');
    expect(names).toContain('RDP-Out');
    expect(rows.filter((r) => r.numericPort === 3389)).toHaveLength(2);

    // Each row must carry its own direction, not the other rule's.
    const inRow = rows.find((r) => r.firewallName === 'RDP-In')!;
    const outRow = rows.find((r) => r.firewallName === 'RDP-Out')!;
    expect(inRow.firewallDirection).toBe('Inbound');
    expect(outRow.firewallDirection).toBe('Outbound');
  });

  test('a single firewall rule still merges into the matching honeypot row', () => {
    const ports: PortEntry[] = [{ port: 3389, label: 'RDP', enabled: true, custom: true }];
    const blocks: FirewallBlock[] = [fw({ Name: 'RDP-In', Port: '3389', Direction: 'Inbound' })];

    const rows = mergeEntries(ports, blocks, false, new Set(), new Set());

    expect(rows).toHaveLength(1);
    expect(rows[0].honeypot).toBe(true);
    expect(rows[0].firewall).toBe(true);
    expect(rows[0].firewallName).toBe('RDP-In');
    expect(rows[0].firewallDirection).toBe('Inbound');
  });

  test('two rules on the same port do not merge into the honeypot row either', () => {
    const ports: PortEntry[] = [{ port: 3389, label: 'RDP', enabled: true, custom: true }];
    const blocks: FirewallBlock[] = [
      fw({ Name: 'RDP-In', Port: '3389', Direction: 'Inbound' }),
      fw({ Name: 'RDP-Out', Port: '3389', Direction: 'Outbound' }),
    ];

    const rows = mergeEntries(ports, blocks, false, new Set(), new Set());

    // Honeypot row stays firewall-less; both firewall rules stand alone.
    const hpRow = rows.find((r) => r.honeypot)!;
    expect(hpRow.firewall).toBe(false);
    expect(rows.filter((r) => r.firewall)).toHaveLength(2);
  });

  test('multi-port firewall rules (ranges/lists) always stand alone', () => {
    const blocks: FirewallBlock[] = [fw({ Name: 'BitTorrent', Port: '6881-6889', Direction: 'Both' })];
    const rows = mergeEntries([], blocks, false, new Set(), new Set());
    expect(rows).toHaveLength(1);
    expect(rows[0].numericPort).toBeNull();
    expect(rows[0].portDisplay).toBe('6881-6889');
  });
});

describe('managed-port populated-state accessibility', () => {
  test('labels per-row watch and remove controls with the port identity', async () => {
    const source = readFileSync('src/panels/network/PortGuardSection.tsx', 'utf8');
    expect(source).toContain("Port Watch for ${row.label} on ${row.portDisplay}");
    expect(source).toContain("Remove ${row.label} on ${row.portDisplay}");
  });
});
