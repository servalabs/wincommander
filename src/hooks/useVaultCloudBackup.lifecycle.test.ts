import { expect, test } from "bun:test";

declare const Bun: {
  which(name: string): string | null;
  spawn(args: string[], options: { stdout: "pipe"; stderr: "pipe" }): {
    stderr: ReadableStream<Uint8Array>; exited: Promise<number>; kill(): void;
  };
};

test("slow backup status keeps setup editable and cannot overwrite a newer user action", async () => {
  const script = String.raw`
    import { mock } from 'bun:test';
    import { strict as assert } from 'node:assert';
    import { JSDOM } from 'jsdom';
    const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost' });
    globalThis.window = dom.window; globalThis.document = dom.window.document;
    globalThis.navigator = dom.window.navigator; globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const React = await import('react'); const { act, createElement: h } = React;
    const { createRoot } = await import('react-dom/client');
    let tick; window.setInterval = callback => { tick = callback; return 1; };
    let now = 0; Date.now = () => now;
    window.clearInterval = () => {};
    const base = { mount_receipt: 'a'.repeat(64), engine_ready: true, connected: false,
      configured: false, automatic: false, watching: false, running: false,
      can_configure: true, configuration: null, last_run: null, reason_code: null };
    const pending = { ...base, reason_code: 'cloud_backup_sign_in_pending' };
    const connected = { ...base, connected: true };
    let statusCalls = 0; const waiting = []; const setups = [];
    const backend = {
      manageVaultCloudBackup: async request => {
        if (request.action === 'stop') return { ...base, engine_ready: false };
        assert.equal(request.action, 'status'); statusCalls++;
        if (statusCalls === 1) return base;
        return new Promise(resolve => waiting.push(resolve));
      },
      setupVaultCloudBackup: async request => {
        setups.push(request.action);
        return request.action === 'connect' ? pending : { ...connected, configured: true, configuration: request.configuration };
      },
    };
    mock.module('./src/hooks/useBackend', () => ({ default: () => backend }));
    mock.module('@/components/ui/bp', () => ({ Button: ({ children, onClick, disabled }) => h('button', { onClick, disabled }, children) }));
    mock.module('@/components/ui/input', () => ({ Input: props => h('input', props) }));
    const { default: useBackup } = await import('./src/hooks/useVaultCloudBackup');
    const { default: Form } = await import('./src/components/shared/VaultCloudBackupForm');
    let current;
    function Harness() {
      current = useBackup(true, true, 4);
      return h(React.Fragment, null,
        h('button', { disabled: current.busy, onClick: () => void current.perform('connect') }, 'Connect'),
        h('button', { disabled: current.busy, onClick: () => void current.perform('stop') }, 'Stop'),
        current.result && h(Form, { configuration: current.result.configuration,
          disabled: current.busy || !current.result.can_configure,
          canSave: current.result.connected, onSave: config => void current.perform('configure', config) }));
    }
    const host = document.getElementById('root'); const root = createRoot(host);
    const button = text => [...host.querySelectorAll('button')].find(item => item.textContent === text);
    await act(async () => root.render(h(Harness)));
    assert.equal(current.busy, false);
    await act(async () => tick());
    assert.equal(statusCalls, 1, 'disconnected setup must not poll');
    await act(async () => button('Connect').click());
    assert.deepEqual(setups, ['connect']);
    assert.equal(current.busy, false, 'follow-up status must not block controls');
    assert.equal(current.result.reason_code, 'cloud_backup_sign_in_pending');
    assert.equal(host.querySelector('fieldset').disabled, false);
    assert.equal(button('Stop').disabled, false);
    const staleConnect = waiting.shift();
    await act(async () => button('Stop').click());
    assert.equal(current.busy, false);
    await act(async () => waiting.shift()(connected));
    await act(async () => staleConnect(base));
    assert.equal(current.result.connected, true, 'late old status must be ignored');
    const idleCalls = statusCalls;
    now += 3000;
    await act(async () => tick());
    assert.equal(statusCalls, idleCalls, 'idle connections should not poll every three seconds');
    now += 27000;
    await act(async () => tick());
    const stalePoll = waiting.shift();
    assert.equal(current.busy, false);
    assert.equal(host.querySelector('fieldset').disabled, false);
    const name = host.querySelector('input[maxlength="128"]');
    const setValue = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set;
    await act(async () => { setValue.call(name, 'Edited during refresh'); name.dispatchEvent(new window.Event('input', { bubbles: true })); });
    const checkboxes = host.querySelectorAll('input[type="checkbox"]');
    await act(async () => checkboxes[checkboxes.length - 1].click());
    assert.equal(button('Save backup setup').disabled, false);
    await act(async () => button('Save backup setup').click());
    assert.deepEqual(setups, ['connect', 'configure']);
    assert.equal(current.result.configured, true);
    assert.equal(current.result.configuration.remote_folder_name, 'Edited during refresh');
    await act(async () => stalePoll(base));
    assert.equal(current.result.configured, true);
    await act(async () => waiting.shift()({ ...current.result, running: true }));
    const activeCalls = statusCalls;
    now += 3000;
    await act(async () => tick());
    assert.equal(statusCalls, activeCalls + 1, 'active operation completion should refresh promptly');
    assert.equal(current.busy, false, 'active status refresh must not disable controls');
    await act(async () => waiting.shift()({ ...current.result, running: false }));
    await act(async () => root.unmount()); dom.window.close();
  `;
  const child = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  const timer = setTimeout(() => child.kill(), 30_000);
  try {
    const stderr = await new Response(child.stderr).text();
    expect({ code: await child.exited, stderr }).toEqual({ code: 0, stderr: "" });
  } finally { clearTimeout(timer); }
}, 40_000);

test("closing a dialog during an active request never sends stop", async () => {
  const script = String.raw`
    import { mock } from 'bun:test'; import { strict as assert } from 'node:assert'; import { JSDOM } from 'jsdom';
    const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost' });
    globalThis.window = dom.window; globalThis.document = dom.window.document; globalThis.navigator = dom.window.navigator; globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const React = await import('react'); const { act, createElement: h } = React; const { createRoot } = await import('react-dom/client');
    const calls = []; let finish; let current;
    const status = { mount_receipt: 'a'.repeat(64), engine_ready: true, connected: true, configured: true, automatic: false, watching: false, running: false, can_configure: true, configuration: null, last_run: null, reason_code: null };
    const backend = { manageVaultCloudBackup: async req => { calls.push(req.action); if (req.action === 'start') return new Promise(resolve => { finish = resolve; }); return status; } };
    mock.module('./src/hooks/useBackend', () => ({ default: () => backend }));
    const { default: useBackup } = await import('./src/hooks/useVaultCloudBackup');
    function Harness({ open }) { current = useBackup(open, true, 4); return null; }
    const root = createRoot(document.getElementById('root'));
    await act(async () => root.render(h(Harness, { open: true })));
    await act(async () => { void current.perform('start'); });
    assert.equal(current.busy, true);
    await act(async () => root.render(h(Harness, { open: false })));
    assert.deepEqual(calls, ['status', 'status', 'start']);
    await act(async () => finish({ ...status, running: true }));
    assert.deepEqual(calls, ['status', 'status', 'start'], 'closing must not send stop or cancel the backend request');
    await act(async () => root.unmount()); dom.window.close();
  `;
  const child = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  const timer = setTimeout(() => child.kill(), 30_000);
  try { const stderr = await new Response(child.stderr).text(); expect({ code: await child.exited, stderr }).toEqual({ code: 0, stderr: "" }); }
  finally { clearTimeout(timer); }
}, 40_000);

test("initial snapshot unlocks setup before the mount-bound network check and Stop supersedes it", async () => {
  const script = String.raw`
    import { mock } from 'bun:test'; import { strict as assert } from 'node:assert'; import { JSDOM } from 'jsdom';
    const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost' });
    globalThis.window = dom.window; globalThis.document = dom.window.document; globalThis.navigator = dom.window.navigator; globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const React = await import('react'); const { act, createElement: h } = React; const { createRoot } = await import('react-dom/client');
    const receipt = 'a'.repeat(64); let current, finishObservation, statusCount = 0; const calls = [];
    const snapshot = { mount_receipt: receipt, engine_ready: true, connected: true, configured: true, automatic: true, watching: true, running: false, can_configure: true, configuration: null, last_run: null, reason_code: null };
    const backend = { manageVaultCloudBackup: async req => {
      calls.push({ action: req.action, receipt: req.expected_mount_receipt });
      if (req.action === 'stop') return { ...snapshot, watching: false };
      if (++statusCount === 1) return snapshot;
      if (statusCount === 2) return new Promise(resolve => { finishObservation = resolve; });
      return { ...snapshot, watching: false };
    } };
    mock.module('./src/hooks/useBackend', () => ({ default: () => backend }));
    const { default: useBackup } = await import('./src/hooks/useVaultCloudBackup');
    function Harness({ open }) { current = useBackup(open, true, 4); return null; }
    const root = createRoot(document.getElementById('root'));
    await act(async () => root.render(h(Harness, { open: true })));
    assert.equal(current.result.mount_receipt, receipt);
    assert.equal(current.busy, false, 'a delayed network check must not block setup');
    assert.deepEqual(calls.slice(0, 2), [{ action: 'status', receipt: null }, { action: 'status', receipt }]);
    await act(async () => { await current.perform('stop'); });
    assert.equal(current.result.watching, false);
    await act(async () => finishObservation({ ...snapshot, watching: true }));
    assert.equal(current.result.watching, false, 'late observation must not undo Stop');
    assert.equal(calls.filter(call => call.action === 'stop').length, 1);
    await act(async () => root.unmount()); dom.window.close();
  `;
  const child = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  const timer = setTimeout(() => child.kill(), 30_000);
  try { const stderr = await new Response(child.stderr).text(); expect({ code: await child.exited, stderr }).toEqual({ code: 0, stderr: "" }); }
  finally { clearTimeout(timer); }
}, 40_000);

test("closing or switching Vaults leaves a check running without applying its stale result", async () => {
  const script = String.raw`
    import { mock } from 'bun:test'; import { strict as assert } from 'node:assert'; import { JSDOM } from 'jsdom';
    const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost' });
    globalThis.window = dom.window; globalThis.document = dom.window.document; globalThis.navigator = dom.window.navigator; globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const React = await import('react'); const { act, createElement: h } = React; const { createRoot } = await import('react-dom/client');
    let current; const waiting = new Map(), calls = [];
    const snapshot = drive => ({ mount_receipt: (drive === 4 ? 'a' : 'b').repeat(64), engine_ready: true, connected: true, configured: true, automatic: false, watching: false, running: false, can_configure: true, configuration: null, last_run: null, reason_code: null });
    const backend = { manageVaultCloudBackup: async req => {
      calls.push(req.action);
      if (!req.expected_mount_receipt) return snapshot(req.internal_drive);
      return new Promise(resolve => waiting.set(req.internal_drive, resolve));
    } };
    mock.module('./src/hooks/useBackend', () => ({ default: () => backend }));
    const { default: useBackup } = await import('./src/hooks/useVaultCloudBackup');
    function Harness({ open, drive }) { current = useBackup(open, true, drive); return null; }
    const root = createRoot(document.getElementById('root'));
    await act(async () => root.render(h(Harness, { open: true, drive: 4 })));
    await act(async () => root.render(h(Harness, { open: true, drive: 5 })));
    assert.equal(current.result.mount_receipt, 'b'.repeat(64));
    await act(async () => waiting.get(4)({ ...snapshot(4), reason_code: 'stale' }));
    assert.equal(current.result.mount_receipt, 'b'.repeat(64));
    await act(async () => root.render(h(Harness, { open: false, drive: 5 })));
    await act(async () => waiting.get(5)({ ...snapshot(5), reason_code: null }));
    assert.equal(calls.filter(action => action === 'stop').length, 0, 'closing must not stop the check or backup');
    assert.equal(current.result, null, 'a closed dialog must not adopt a late result');
    await act(async () => root.unmount()); dom.window.close();
  `;
  const child = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  const timer = setTimeout(() => child.kill(), 30_000);
  try { const stderr = await new Response(child.stderr).text(); expect({ code: await child.exited, stderr }).toEqual({ code: 0, stderr: "" }); }
  finally { clearTimeout(timer); }
}, 40_000);

test("switching Vaults shows loading until the new status is observed", async () => {
  const script = String.raw`
    import { mock } from 'bun:test'; import { strict as assert } from 'node:assert'; import { JSDOM } from 'jsdom';
    const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost' });
    globalThis.window = dom.window; globalThis.document = dom.window.document; globalThis.navigator = dom.window.navigator; globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const React = await import('react'); const { act, createElement: h } = React; const { createRoot } = await import('react-dom/client');
    let current, finish;
    const status = { mount_receipt: 'a'.repeat(64), engine_ready: true, connected: true, configured: false, automatic: false, watching: false, running: false, can_configure: true, configuration: null, last_run: null, reason_code: null };
    const backend = { manageVaultCloudBackup: async req => req.internal_drive === 4 ? status : new Promise(resolve => { finish = resolve; }) };
    mock.module('./src/hooks/useBackend', () => ({ default: () => backend }));
    const { default: useBackup } = await import('./src/hooks/useVaultCloudBackup');
    function Harness({ drive }) { current = useBackup(true, true, drive); return null; }
    const root = createRoot(document.getElementById('root'));
    await act(async () => root.render(h(Harness, { drive: 4 })));
    assert.equal(current.result.mount_receipt, status.mount_receipt);
    await act(async () => root.render(h(Harness, { drive: 5 })));
    assert.equal(current.result, null); assert.equal(current.busy, true); assert.equal(current.action, 'status');
    await act(async () => finish({ ...status, mount_receipt: 'b'.repeat(64) }));
    assert.equal(current.busy, false); assert.equal(current.result.mount_receipt, 'b'.repeat(64));
    await act(async () => root.unmount()); dom.window.close();
  `;
  const child = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  const timer = setTimeout(() => child.kill(), 30_000);
  try { const stderr = await new Response(child.stderr).text(); expect({ code: await child.exited, stderr }).toEqual({ code: 0, stderr: "" }); }
  finally { clearTimeout(timer); }
}, 40_000);
