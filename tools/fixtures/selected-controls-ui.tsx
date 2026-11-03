// Browser-only synthetic state. Every native call is intercepted below.
import React from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import '../../src/index.css';
import '../../src/styles/v2-theme.css';
import { MotionPreferenceProvider } from '../../src/hooks/useMotionPreference';
import { AppConfirmProvider } from '../../src/components/shared/AppConfirmDialog';
import UpdateRestartCard from '../../src/components/tweaks/UpdateRestartCard';
import ShellVisibilityCard from '../../src/components/tweaks/ShellVisibilityCard';
import ContextItemVisibilityCard from '../../src/components/tweaks/ContextItemVisibilityCard';
import AdditionalAppPermissions from '../../src/panels/privacy/AdditionalAppPermissions';
import IpFirewallCard from '../../src/panels/network/IpFirewallCard';
import PortGuardHistory from '../../src/panels/network/PortGuardHistory';
import ActivePorts from '../../src/components/network/ActivePorts';
import type { PortGuardHit, PortGuardStatus } from '../../src/lib/portGuard';

const fixture = window as typeof window & { __TAURI_INTERNALS__: unknown; selectedCalls: {command:string;params:Record<string,string>}[]; copiedReport: string };
const fixtureMode = new URLSearchParams(location.search);
const limited = fixtureMode.has('limited');
fixture.selectedCalls = []; fixture.copiedReport = '';
Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async (text: string) => { fixture.copiedReport = text; } } });
let restart = { isAdmin: !limited, configured: false, policyExists: false, owned: false, reserved: false, scheduledMode: false, canPrepareSchedule: true, scheduleOwned: false, scheduleReserved: false, conflict: false };
const shell = ['StartRecommended', 'SettingsHome'].map(item => ({ item, eligible: true, owned: false, pending: false, conflict: false, userPolicy: false, existing: false, externalHidden: false, canApply: true }));
const context = ['IncludeLibrary', 'GiveAccess', 'Share'].map(item => ({ item, detected: 1, owned: false, pending: false, conflict: false, otherAccount: false }));
const caps = ['humanPresence','passkeys','bluetooth','humanInterfaceDevice','passkeysEnumeration','sensors.custom','serialCommunication','usb','wifiData','wiFiDirect'].flatMap(capability => ['User','Machine'].map(scope => ({ capability, scope, available: true, value: 'Allow', owned: false, pending: false, conflict: false, managed: false })));
if (limited) { shell[0].eligible = false; shell[1].userPolicy = true; context[0].conflict = true; context[2].detected = 0; caps.filter(row => row.capability === 'humanPresence').forEach(row => { row.available = false; }); }
if (fixtureMode.has('pending')) { shell[0].pending = true; const row = caps.find(item => item.capability === 'bluetooth' && item.scope === 'User'); if (row) row.pending = true; }
let ipRules: {Id:string;RemoteAddress:string;Direction:string;Action:string;Enabled:boolean;Active:boolean;ScopeMatches:boolean}[] = [];
fixture.__TAURI_INTERNALS__ = {
  transformCallback: () => 1, unregisterCallback: () => {},
  invoke: async (command: string, args: Record<string, unknown> = {}) => {
    if (command === 'get_license_status') return { valid: true, features: fixtureMode.has('investigator') ? ['paid','advanced'] : ['paid'], trial_active: false };
    if (command === 'plugin:event|listen') return 1;
    if (command === 'plugin:event|unlisten') return null;
    if (command !== 'run_backend_script') throw new Error(`Fixture refuses native call: ${command}`);
    const name = String(args.command); const params = (args.params || {}) as Record<string, string>;
    fixture.selectedCalls.push({command:name,params});
    if (name === 'Get-NetworkPorts') return {status:'ok',durationMs:1,truncated:false,totals:{tcp:1,udp:0,shown:1},rows:[{proto:'TCP',localAddr:'192.0.2.1',localPort:54321,remoteAddr:'198.51.100.4',remotePort:443,state:'ESTABLISHED',pid:42,processName:'fixture-browser',processPath:'C:\\private\\fixture.exe'}]};
    if (name === 'Get-UpdateRestartPolicyStatus') return structuredClone(restart);
    if (name === 'Set-UpdateRestartPolicy') {
      restart = params.Mode === 'revert' ? { ...restart, configured:false, policyExists:false, owned:false, scheduledMode:false, canPrepareSchedule:true, scheduleOwned:false } : { ...restart, configured:true, policyExists:true, owned:true, scheduledMode:true, canPrepareSchedule:false, scheduleOwned:params.Mode === 'applyWithSchedule' };
      return structuredClone(restart);
    }
    if (name === 'Get-ShellVisibilityStatus') return {isAdmin:!limited,items:structuredClone(shell)};
    if (name === 'Set-ShellVisibility') { const row=shell.find(item=>item.item===params.Item); if (!row) throw new Error('bad item'); row.owned=params.Mode==='hide'; row.pending=false; return {verified:true}; }
    if (name === 'Get-ContextItemVisibility') return {isAdmin:!limited,items:structuredClone(context)};
    if (name === 'Set-ContextItemVisibility') { const row=context.find(item=>item.item===params.Item); if (!row) throw new Error('bad item'); row.owned=params.Mode==='hide'; row.detected=row.owned?0:1; return {verified:true}; }
    if (name === 'Get-AdditionalAppPermissions') return {isAdmin:!limited,permissions:structuredClone(caps)};
    if (name === 'Set-AdditionalAppPermission') { const row=caps.find(item=>item.capability===params.Capability && item.scope===params.Scope); if (!row) throw new Error('bad item'); row.owned=params.Mode==='deny'; row.pending=false; row.value=row.owned?'Deny':'Allow'; return {verified:true}; }
    if (name === 'Get-IPFirewallRules') return {readbackComplete:true,rules:structuredClone(ipRules)};
    if (name === 'Add-IPFirewallRule') { if (params.Address === 'bad') return {error:true,message:'Invalid IP address'}; ipRules=[{Id:'fixture-rule',RemoteAddress:params.Address,Direction:params.Direction,Action:params.Action,Enabled:true,Active:true,ScopeMatches:true}]; return {verified:true}; }
    if (name === 'Remove-IPFirewallRule') { ipRules=[]; return {verified:true}; }
    throw new Error(`Fixture refuses backend call: ${name}`);
  },
};
const hit: PortGuardHit = {id:1,port:22,protocol:'tcp',service:'SSH',peer:'192.0.2.2:10',localAddress:'192.0.2.1',outcome:'blocked',loopback:false,detectedAt:'2026-01-02T00:00:00Z'};
const portStatus: PortGuardStatus = {schemaVersion:2,running:true,desiredEnabled:true,provider:'fixture',health:'ok',coverage:'limited',coverageComplete:false,coverageNote:'Not all traffic is observed',collectionSettingsRetained:true,collectorHealthy:true,lastError:null,observedAt:'2026-01-02T00:00:01Z',generation:1,observedEvents:1,droppedEvents:2,historyEvicted:1,historyError:null,historyRetention:'bounded'};
document.body.style.cssText = 'margin:0;padding:16px;min-width:0;height:auto;overflow:auto';
document.documentElement.style.cssText = 'height:auto;overflow:auto';
createRoot(document.getElementById('fixture')!).render(<QueryClientProvider client={new QueryClient()}><MotionPreferenceProvider><AppConfirmProvider>
  <main style={{display:'grid',gap:16}}><UpdateRestartCard /><ShellVisibilityCard searchQuery="" /><ContextItemVisibilityCard searchQuery="" /><AdditionalAppPermissions searchQuery="" /><IpFirewallCard /><PortGuardHistory hits={[hit]} status={portStatus} stale={false} readError={null} busy={false} onClear={() => {}} /><ActivePorts /></main>
</AppConfirmProvider></MotionPreferenceProvider></QueryClientProvider>);
