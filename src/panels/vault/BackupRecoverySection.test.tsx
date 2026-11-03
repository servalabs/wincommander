import { expect, test } from "bun:test";

declare const Bun: {
  which(name: string): string | null;
  spawn(args: string[], options: { stdout: "pipe"; stderr: "pipe" }): { stderr: ReadableStream<Uint8Array>; exited: Promise<number>; kill(): void };
};

test("rendered backup pickers dispatch once, survive navigation, and explain recovery outcomes", async () => {
  // Isolate Tauri/auth mocks and the in-memory task singleton from other tests.
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
    const choices = [], dialogs = [], calls = [];
    const slash = String.fromCharCode(92);
    const sourcePath = ['C:', 'Vaults', 'records.hc'].join(slash);
    const copyParent = slash.repeat(2) + ['backup', 'share'].join(slash);
    const generation = ['V:', 'generation'].join(slash), restoreParent = ['C:', 'private'].join(slash);
    mock.module('@tauri-apps/plugin-dialog', () => ({ open: async options => { dialogs.push(options); return choices.shift(); } }));
    mock.module('./src/components/shared/TierGate', () => ({ default: ({ children }) => children }));
    mock.module('./src/components/shared/SectionCard', () => ({ default: ({ title, children }) => h('section', null, h('h2', null, title), children) }));
    mock.module('@/components/ui/bp', () => ({
      Button: ({ children, disabled, onClick, ...rest }) => h('button', { disabled, onClick, 'aria-label': rest['aria-label'] }, children),
      Callout: ({ title, children }) => h('div', null, title && h('h3', null, title), children),
      Checkbox: ({ label, ...props }) => h('label', null, h('input', { type: 'checkbox', ...props }), label),
      Icon: () => null, ProgressBar: () => null, Spinner: () => null, Tooltip: ({ children }) => children,
    }));
    let complete;
    const backend = {
      exportEncryptedBackup: (...args) => { calls.push(['copy', ...args]); return new Promise(resolve => { complete = resolve; }); },
      verifyEncryptedBackup: async (...args) => { calls.push(['check', ...args]); return { success: false, error: 'backup_rejected: incomplete private-path' }; },
      restoreBackupGeneration: async (...args) => { calls.push(['restore', ...args]); return args[2] ? { success: true, data: { operation: 'restored', bytes: '15', files: 1, legacy: true, directoryName: 'WinCommander-Restored-Files-00000000-0000-4000-8000-000000000001' } } : { success: false, error: 'backup_legacy_required' }; },
    };
    mock.module('./src/hooks/useBackend', () => ({ default: () => backend }));
    const { default: Panel } = await import('./src/panels/vault/BackupRecoverySection');
    const host = document.getElementById('root');
    let root = createRoot(host);
    const button = label => [...host.querySelectorAll('button')].find(node => node.textContent === label);
    const click = async label => { assert.ok(button(label), label); await act(async () => { button(label).click(); }); };
    await act(async () => { root.render(h(Panel)); });
    assert.equal(button('Copy and check container').disabled, true);
    choices.push(sourcePath, copyParent);
    await click('Existing container…'); await click('Backup location…');
    assert.deepEqual(dialogs.map(value => value.directory), [false, true]);
    await click('Copy and check container');
    assert.deepEqual(calls, [['copy', sourcePath, copyParent]]);
    assert.match(host.querySelector('[role="status"]').textContent, /Keep WinCommander open/);
    assert.equal(button('Copy and check container').disabled, true);
    await act(async () => { root.unmount(); });
    root = createRoot(host);
    await act(async () => { root.render(h(Panel)); });
    assert.match(host.querySelector('[role="status"]').textContent, /Copying and checking/);
    assert.equal(calls.length, 1);
    await act(async () => { complete({ success: true, data: { operation: 'copied', bytes: '123', legacy: false, directoryName: 'WinCommander-Container-Copy-00000000-0000-4000-8000-000000000001' } }); });
    assert.match(host.querySelector('[role="status"]').textContent, /123 bytes checked/);
    assert.ok(host.textContent.includes(copyParent + slash + 'WinCommander-Container-Copy-'));
    choices.push(['D:', 'retrieved'].join(slash)); await click('Exported copy folder…'); await click('Check copy');
    assert.match(host.querySelector('[role="alert"]').textContent, /Incomplete files may remain/);
    assert.ok(!host.textContent.includes('private-path'));
    choices.push(generation, restoreParent);
    await click('Generation folder in the mounted copy…'); await click('Restore location…');
    await click('Check and restore files');
    assert.deepEqual(calls.at(-1), ['restore', generation, restoreParent, false]);
    assert.match(host.querySelector('[role="alert"]').textContent, /Select legacy recovery explicitly/);
    await act(async () => { host.querySelector('input[type="checkbox"]').click(); });
    await click('Check and restore files');
    assert.deepEqual(calls.at(-1), ['restore', generation, restoreParent, true]);
    assert.match(host.querySelector('[role="status"]').textContent, /Destination contains plaintext/);
    assert.match(host.querySelector('[role="status"]').textContent, /original hashes were unavailable/);
    await act(async () => { root.unmount(); });
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
