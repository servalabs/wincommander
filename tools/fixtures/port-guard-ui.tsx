// SPDX-License-Identifier: AGPL-3.0-or-later
// Browser-only synthetic data. No native bridge or Windows mutation is available.
import React from 'react';
import { createRoot } from 'react-dom/client';
import '../../src/index.css';
import '../../src/styles/v2-theme.css';
import PortGuardSection from '../../src/panels/network/PortGuardSection';
import { MotionPreferenceProvider } from '../../src/hooks/useMotionPreference';

const fixtureWindow = window as typeof window & { __TAURI_INTERNALS__: unknown; guardFixtureCalls: unknown[] };
fixtureWindow.guardFixtureCalls = [];
let watches = [
  { port: 7070, endPort: 7070, protocol: 'tcp', label: 'AnyDesk direct', enabled: true, custom: true },
  { port: 3306, endPort: 3306, protocol: 'tcp', label: 'Database_' + 'LongLabel'.repeat(6), enabled: true, custom: true },
  { port: 443, endPort: 443, protocol: 'tcp', label: 'AnyDesk relay', enabled: true, custom: true },
];
const makeHit = (id: number, seconds: number, outcome = 'blocked') => ({ id, port: 3306, protocol: 'tcp', service: watches[1].label,
  peer: '[2001:db8:abcd:ef01::2]:55000', localAddress: '2001:db8:abcd:ef01::1', outcome, loopback: false,
  detectedAt: new Date(Date.UTC(2026, 0, 1, 12, 0, seconds)).toISOString() });
let hits = [makeHit(1, 0), makeHit(2, 1), makeHit(3, 3), makeHit(4, 7), makeHit(5, 8, 'allowed')];
let blocks = [{ Name: 'LongFirewallLabel'.repeat(3), Port: '7070,8000-8010', Protocol: 'TCP', Direction: 'Both', Enabled: 1, Verified: true, Issues: [] as string[] }];
fixtureWindow.__TAURI_INTERNALS__ = {
  transformCallback: () => 1, unregisterCallback: () => {},
  invoke: async (command: string, args: Record<string, unknown> = {}) => {
    fixtureWindow.guardFixtureCalls.push({ command, args });
    if (command === 'plugin:event|listen') return 1;
    if (command === 'plugin:event|unlisten') return null;
    if (command === 'get_ping_block_status') return { blocked: false };
    if (command === 'network_honeypot_status') return {
      schemaVersion: 2, desiredEnabled: true, running: true, coverageComplete: false, collectorHealthy: true,
      health: 'degraded', observedAt: new Date().toISOString(), observedEvents: 5, droppedEvents: 0, historyEvicted: 0,
      collectionSettingsRetained: true, historyError: null, lastError: null,
      coverageNote: 'Windows allow/drop classifications. Existing connections and outbound relay sessions are not new inbound attempts.',
    };
    if (command === 'get_network_honeypot_ports') return structuredClone(watches);
    if (command === 'get_network_honeypot_recent') return structuredClone(hits);
    if (command === 'clear_network_honeypot_recent') { hits = []; return null; }
    if (command === 'remove_network_honeypot_custom_port') { watches = watches.filter(rule => rule.port !== args.port); return null; }
    if (command === 'run_backend_script' && args.command === 'Get-ProtocolBlocks') return { blocks: structuredClone(blocks) };
    if (command === 'run_backend_script' && args.command === 'Block-Protocol') {
      blocks[0].Verified = false; blocks[0].Issues = ['Synthetic mismatched firewall read-back'];
      return { error: 'Synthetic creation failed verification' };
    }
    if (command === 'run_backend_script' && args.command === 'Unblock-Protocol') { blocks = []; return { status: 'removed', verified: true }; }
    throw new Error(`Synthetic fixture refuses mutation or unsupported call: ${command}`);
  },
};
document.body.style.cssText = 'margin:0; padding:16px; min-width:0; height:auto; overflow:auto';
document.documentElement.style.cssText = 'height:auto;overflow:auto';
createRoot(document.getElementById('fixture')!).render(<MotionPreferenceProvider><h1 style={{ fontSize: 14, marginBottom: 12 }}>Isolated UI test — synthetic records, no Windows changes</h1><PortGuardSection /></MotionPreferenceProvider>);
