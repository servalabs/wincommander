import { expect, test } from "bun:test";

declare const Bun: {
  which(name: string): string | null;
  spawn(args: string[], options: { stdout: "pipe"; stderr: "pipe" }): { stderr: ReadableStream<Uint8Array>; exited: Promise<number>; kill(): void };
};

test("another Windows profile sees active retry, terminal failure, then recovered connection", async () => {
  // Keep the rendered-view mocks and fake polling clock separate from other tests.
  const script = String.raw`
    import { mock } from 'bun:test';
    import { strict as assert } from 'node:assert';
    import { JSDOM } from 'jsdom';
    const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost' });
    globalThis.window = dom.window;
    globalThis.document = dom.window.document;
    globalThis.navigator = dom.window.navigator;
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const React = await import('react');
    const { act, createElement: h } = React;
    const { createRoot } = await import('react-dom/client');
    const timers = new Map(); let nextTimer = 0;
    globalThis.setInterval = (callback, delay) => { const id = ++nextTimer; timers.set(id, { callback, delay }); return id; };
    globalThis.clearInterval = id => timers.delete(id);
    let status = { connected: false, deviceId: 'device-123', serverUrl: 'https://fleet.example.test', lastEnrollAt: null, lastError: 'server temporarily unavailable', retrying: true };
    const calls = []; const connectArgs = []; let connectError = null; let connectGate = null; let trustMatches = null; let dispatchExtra = {}; const dispatchCalls = [];
    const savedFleet = () => ({ dispatch: true, signingKeyPub: 'KEY', serverUrl: '', enabled: false });
    let settingsFleet = savedFleet();
    mock.module('@tauri-apps/api/core', () => ({ invoke: async (command, args) => {
      calls.push(command);
      if (command === 'fleet_connect') { connectArgs.push(args); if (connectError) throw connectError; if (connectGate) await connectGate; return {}; }
      if (command === 'fleet_status') return { ...status };
      if (command === 'fleet_machine_trust_status') return { trustBound: true, shieldPolicyStatus: 'unverified', trustMatchesAgent: trustMatches };
      if (command === 'fleet_set_dispatch') { dispatchCalls.push(args); return { dispatch: args.enabled, changed: true, ...dispatchExtra }; }
      if (command === 'fleet_service_contact_status') return { observation: null };
      throw new Error('unexpected mutation: ' + command);
    } }));
    mock.module('@/hooks/useSettings', () => ({ getSettingsOnce: async () => ({ app: { fleet: settingsFleet }, deviceId: 'device-123' }) }));
    mock.module('@/hooks/queries/useLicenseQuery', () => ({ useLicenseQuery: () => ({ data: { valid: true, features: ['fleet'] }, isLoading: false }) }));
    mock.module('@/components/ui/button', () => ({ Button: ({ children, variant, size, ...props }) => h('button', props, children) }));
    mock.module('@/components/ui/input', () => ({ Input: props => h('input', props) }));
    mock.module('@/components/ui/switch', () => ({ Switch: ({ checked, onCheckedChange, ...props }) => h('input', { ...props, type: 'checkbox', checked, onChange: event => onCheckedChange(event.target.checked) }) }));
    mock.module('@/components/ui/icon', () => ({ Icon: () => null }));
    const { default: View } = await import('./src/panels/fleet/FleetConnectView');
    const host = document.getElementById('root'), root = createRoot(host);
    const connect = () => [...host.querySelectorAll('button')].find(button => button.textContent === 'Connect to Fleet');
    const poll = async () => { await act(async () => { [...timers.values()].find(timer => timer.delay === 4000).callback(); }); };
    await act(async () => { root.render(h(View)); });
    assert.match(host.textContent, /Reconnecting/);
    assert.match(host.textContent, /server temporarily unavailable/);
    assert.ok(!connect(), 'active retry must not offer another enrollment');
    const change = [...host.querySelectorAll('button')].find(button => button.textContent === 'Change Fleet server');
    assert.ok(change, 'an explicit address change must be available while retrying');
    await act(async () => { change.click(); });
    assert.ok(connect(), 'explicit address change opens the existing connection form');
    assert.ok(host.querySelector('input[placeholder="https://fleet.example.com"]'));
    await act(async () => { [...host.querySelectorAll('button')].find(button => button.textContent === 'Cancel').click(); });
    assert.ok(!connect(), 'cancelling returns to the existing retry');
    assert.ok(!host.textContent.includes('stays enrolled'), 'retry alone cannot establish completed enrollment');
    status = { ...status, retrying: false, lastError: 'enrollment rejected' };
    await poll();
    assert.match(host.textContent, /Connection error/);
    assert.match(host.textContent, /enrollment rejected/);
    assert.ok(connect(), 'terminal failure permits explicit enrollment');
    status = { ...status, connected: true, retrying: true, lastError: null };
    await poll();
    assert.match(host.querySelector('.fleet-connect-badge').textContent, /Linked/);
    assert.ok(!connect());
    assert.ok(!host.textContent.includes('enrollment rejected'));
    status = { ...status, pendingApproval: true, admissionFingerprint: 'a'.repeat(64) };
    await poll();
    assert.match(host.textContent, /Device identity fingerprint/);
    assert.ok(host.textContent.includes('a'.repeat(64)));
    status = { ...status, admissionFingerprint: null };
    await poll();
    assert.match(host.textContent, /fingerprint is unavailable/);
    assert.ok(calls.every(command => ['fleet_status', 'fleet_machine_trust_status', 'fleet_service_contact_status'].includes(command)));
    // Dispatch is visible and switchable without re-enrolling.
    const dispatchSwitch = () => host.querySelector('input[aria-label="Remote command dispatch"]');
    assert.match(host.textContent, /Remote command dispatch: ON/);
    assert.ok(dispatchSwitch().checked && !dispatchSwitch().disabled);
    dispatchExtra = { agentReconfigured: false };
    await act(async () => { dispatchSwitch().click(); });
    assert.match(host.textContent, /saved as OFF, but the running Fleet agent could not be updated/);
    dispatchExtra = {};
    assert.deepEqual(dispatchCalls, [{ enabled: false }]);
    assert.match(host.textContent, /Remote command dispatch: OFF/);
    assert.match(host.textContent, /stay queued and are not executed/);
    assert.ok(!calls.includes('fleet_connect'), 'toggling dispatch must not enroll again');
    await act(async () => { dispatchSwitch().click(); });
    assert.deepEqual(dispatchCalls, [{ enabled: false }, { enabled: true }]);
    assert.match(host.textContent, /Remote command dispatch: ON/);
    // A service trust that disagrees with the agent, or a failed bridge, shows a repair path.
    const refreshTrust = async () => act(async () => { [...timers.values()].find(timer => timer.delay === 15000).callback(); });
    assert.ok(!host.textContent.includes('Service trust needs repair'));
    trustMatches = false;
    await refreshTrust();
    assert.match(host.textContent, /Service trust needs repair/);
    assert.ok([...host.querySelectorAll('button')].some(button => button.textContent === 'Repair machine trust'));
    trustMatches = null;
    await refreshTrust();
    assert.ok(!host.textContent.includes('Service trust needs repair'));
    status = { ...status, trustBridgeError: 'identity_change_unconfirmed' };
    await poll();
    assert.match(host.textContent, /Service trust needs repair/);
    assert.match(host.textContent, /differs from the one this device's service trusts/);
    // A non-repairable code is information only: no repair button.
    status = { ...status, trustBridgeError: 'proof_timeout' };
    await poll();
    assert.match(host.textContent, /Service trust: The Fleet agent did not finish enrolling/);
    assert.ok(![...host.querySelectorAll('button')].some(button => button.textContent === 'Repair machine trust'));
    status = { ...status, trustBridgeError: 'binding_changed' };
    await poll();
    assert.match(host.textContent, /Service trust needs repair/);
    status = { ...status, trustBridgeError: null };
    await poll();
    status = { ...status, connected: false, retrying: true, lastError: 'connection unavailable' };
    await poll();
    await act(async () => { [...host.querySelectorAll('button')].find(button => button.textContent === 'Change Fleet server').click(); });
    const input = host.querySelector('input[placeholder="https://fleet.example.com"]');
    await act(async () => {
      Object.getOwnPropertyDescriptor(dom.window.HTMLInputElement.prototype, 'value').set.call(input, 'https://fleet.example.test:9443');
      input.dispatchEvent(new dom.window.Event('input', { bubbles: true }));
    });
    await act(async () => { connect().click(); });
    assert.equal(connectArgs.length, 1);
    assert.equal(connectArgs[0].serverUrl, 'https://fleet.example.test:9443');
    assert.equal(connectArgs[0].dispatch, true, 'Connect preserves the saved dispatch value');
    assert.equal(connectArgs[0].signingKeyPub, 'KEY', 'Connect sends the saved signing key');
    status = { ...status, retrying: false, lastError: 'enroll rejected: HTTP 403 Forbidden' };
    await act(async () => { [...timers.values()].find(timer => timer.delay === 1000).callback(); });
    assert.match(host.textContent, /Last agent error \(may be from an earlier attempt\): enroll rejected: HTTP 403 Forbidden/);
    // While enrolling, the address stays editable and the attempt can be retried.
    const urlInput = () => host.querySelector('input[placeholder="https://fleet.example.com"]');
    const retry = () => [...host.querySelectorAll('button')].find(button => button.textContent === 'Retry connection');
    const setUrl = async value => act(async () => {
      Object.getOwnPropertyDescriptor(dom.window.HTMLInputElement.prototype, 'value').set.call(urlInput(), value);
      urlInput().dispatchEvent(new dom.window.Event('input', { bubbles: true }));
    });
    assert.ok(!urlInput().disabled, 'address stays editable while enrolling');
    assert.ok(retry() && !retry().disabled, 'enrolling can be retried');
    // Plain http:// is explained and never reaches the backend.
    await setUrl('http://fleet.example.test:9443');
    assert.match(host.textContent, /Fleet requires an https:\/\/ address/);
    await act(async () => { retry().click(); });
    assert.equal(connectArgs.length, 1, 'http:// must not be sent to fleet_connect');
    // A backend https rejection is shown as the same actionable hint.
    connectError = 'Fleet requires an HTTPS server with post-quantum hybrid TLS support.';
    await setUrl('https://fleet.example.test:9443');
    await act(async () => { retry().click(); });
    assert.equal(connectArgs.length, 2);
    assert.match(host.textContent, /Change http:\/\/ to https:\/\//);
    connectError = null;
    // A retry click while an attempt is still in flight cannot start a second
    // overlapping fleet_connect: the stale node is clicked twice before release.
    let release; connectGate = new Promise(resolve => { release = resolve; });
    const before = connectArgs.length, staleRetry = retry();
    await act(async () => { staleRetry.click(); staleRetry.click(); });
    assert.equal(connectArgs.length, before + 1, 'a second click must not start another connect');
    assert.ok([...host.querySelectorAll('button')].find(button => button.textContent === 'Connecting…').disabled, 'connect is disabled while in flight');
    await act(async () => { release(); connectGate = null; await Promise.resolve(); });
    // Admitted-but-unapproved: clear waiting state, still correctable.
    status = { ...status, connected: false, retrying: true, pendingApproval: true, lastError: null, lastEnrollAt: '2026-11-17T10:00:00Z', admissionFingerprint: null };
    await act(async () => { [...timers.values()].find(timer => timer.delay === 1000).callback(); });
    assert.match(host.textContent, /Waiting for admin approval/);
    assert.match(host.querySelector('.fleet-connect-badge').textContent, /Awaiting approval/);
    assert.ok([...host.querySelectorAll('button')].some(button => button.textContent === 'Change Fleet server'));
    // A stale pendingApproval flag with a current failure is not "waiting".
    status = { ...status, lastError: 'check-in timed out' };
    await act(async () => { [...timers.values()].find(timer => timer.delay === 1000 || timer.delay === 4000).callback(); });
    assert.ok(!host.textContent.includes('Waiting for admin approval'));
    assert.match(host.textContent, /check-in timed out/);
    await act(async () => { root.unmount(); });
    // With no saved signing key, dispatch cannot be enabled and says why.
    settingsFleet = { ...savedFleet(), dispatch: false, signingKeyPub: '' };
    status = { ...status, connected: true, retrying: true, pendingApproval: false, lastError: null };
    const second = createRoot(host);
    await act(async () => { second.render(h(View)); });
    assert.match(host.textContent, /Remote command dispatch: OFF/);
    assert.match(host.textContent, /Dispatch needs the Fleet signing key/);
    assert.ok(dispatchSwitch().disabled);
    await act(async () => { second.unmount(); });
    assert.equal(timers.size, 0);
    dom.window.close();
  `;
  const process = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  let timedOut = false;
  const timer = setTimeout(() => { timedOut = true; process.kill(); }, 30_000);
  try {
    const stderr = await new Response(process.stderr).text();
    expect({ code: await process.exited, stderr, timedOut }).toEqual({ code: 0, stderr: "", timedOut: false });
  } finally { clearTimeout(timer); }
}, 45_000);
