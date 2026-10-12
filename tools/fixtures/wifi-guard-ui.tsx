// SPDX-License-Identifier: AGPL-3.0-or-later
// Run with tools/tests/wifi-guard-ui.cjs, which supplies an isolated app-state module.
import React from 'react';
import { createRoot } from 'react-dom/client';
import '../../src/index.css';
import '../../src/styles/v2-theme.css';
import WifiGuardSection from '../../src/panels/network/WifiGuardSection';
import { AppConfirmProvider } from '../../src/components/shared/AppConfirmDialog';
import { MotionPreferenceProvider } from '../../src/hooks/useMotionPreference';

const mode = new URLSearchParams(location.search);
const fixtureWindow = window as typeof window & { __TAURI_INTERNALS__: unknown; wifiFixtureCalls: string[] };
fixtureWindow.wifiFixtureCalls = [];
fixtureWindow.__TAURI_INTERNALS__ = {
  transformCallback: () => 1, unregisterCallback: () => {},
  invoke: async (command: string) => {
    fixtureWindow.wifiFixtureCalls.push(command);
    if (command === 'plugin:event|listen') return 1;
    if (command === 'plugin:event|unlisten') return null;
    if (command === 'wifi_guard_status') {
      if (mode.has('error')) throw new Error('Synthetic Wi-Fi read failure');
      return { schemaVersion: 2, instanceId: 'fixture', baselineRevision: 1, running: true,
        collectorHealthy: true, health: 'permissionDenied', observedAt: new Date().toISOString(),
        interfaces: [], pollIntervalSecs: 10 };
    }
    if (command === 'get_wifi_guard_recent') return mode.has('alerts') ? [{ id: 'fixture-alert',
      interfaceId: 'fixture', ssid: 'Example network', bssid: '02:00:00:00:00:01',
      reason: 'newBssid', detectedAt: new Date().toISOString() }] : [];
    throw new Error(`Synthetic fixture refuses native mutation: ${command}`);
  },
};
document.body.style.cssText = 'margin:0;padding:16px;min-width:0;height:auto;overflow:auto';
createRoot(document.getElementById('fixture')!).render(<MotionPreferenceProvider><AppConfirmProvider><WifiGuardSection embedded /></AppConfirmProvider></MotionPreferenceProvider>);
