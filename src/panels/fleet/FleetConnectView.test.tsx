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
    const calls = [];
    mock.module('@tauri-apps/api/core', () => ({ invoke: async command => {
      calls.push(command);
      if (command === 'fleet_status') return { ...status };
      if (command === 'fleet_machine_trust_status') return { trustBound: true, shieldPolicyStatus: 'unverified' };
      if (command === 'fleet_service_contact_status') return { observation: null };
      throw new Error('unexpected mutation: ' + command);
    } }));
    mock.module('@/hooks/useSettings', () => ({ getSettingsOnce: async () => ({ app: {}, deviceId: '' }) }));
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
    assert.ok(calls.every(command => ['fleet_status', 'fleet_machine_trust_status', 'fleet_service_contact_status'].includes(command)));
    await act(async () => { root.unmount(); });
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
