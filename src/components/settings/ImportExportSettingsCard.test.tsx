import { expect, test } from "bun:test";

declare const Bun: {
  which(name: string): string | null;
  spawn(args: string[], options: { stdout: "pipe"; stderr: "pipe" }): {
    stderr: ReadableStream<Uint8Array>; exited: Promise<number>; kill(): void;
  };
};

test("password-protected export and encrypted and legacy restore require confirmation", async () => {
  const script = String.raw`
    import { mock } from 'bun:test';
    import { strict as assert } from 'node:assert';
    import { JSDOM } from 'jsdom';
    try {
    const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost' });
    globalThis.window = dom.window; globalThis.document = dom.window.document;
    globalThis.navigator = dom.window.navigator; globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const React = await import('react'); const { act, createElement: h } = React;
    const { createRoot } = await import('react-dom/client');
    let selected = null, exported = [], imported = [], errors = [];
    mock.module('./src/hooks/useSettings', () => ({
      writeSettingsExportFile: async value => { exported.push(value); return true; },
      readSettingsImportFile: async () => selected,
      decryptSettingsImportFile: async (json, password) => { if (password !== 'correct-password') throw new Error('Password incorrect or backup damaged.'); return '{"settingsVersion":2,"app":{},"ideal":{},"current":{},"policy":{}}'; }
    }));
    mock.module('./src/context/AppContext', () => ({ useAppState: () => ({ refreshSettings: async () => {} }) }));
    mock.module('./src/context/SettingsWriteContext', () => ({ useSettingsWriter: () => ({ import: async value => imported.push(value) }) }));
    mock.module('./src/utils/toast', () => ({ showSuccess: () => {}, showError: value => errors.push(value) }));
    mock.module('./src/components/shared/SectionCard', () => ({ default: ({ headerRight, children }) => h('section', null, headerRight, children) }));
    mock.module('@/components/ui/bp', () => ({
      Button: ({ text, onClick, disabled }) => h('button', { onClick, disabled }, text), Icon: () => null,
      Alert: ({ isOpen, onConfirm, children }) => isOpen ? h('div', null, children, h('button', { onClick: onConfirm }, 'Replace settings')) : null
    }));
    const passthrough = ({ children }) => h('div', null, children);
    mock.module('@/components/ui/popover', () => ({ Popover: passthrough, PopoverTrigger: passthrough, PopoverContent: passthrough }));
    mock.module('@/components/ui/tooltip', () => ({ Tooltip: passthrough, TooltipTrigger: passthrough, TooltipContent: passthrough }));
    mock.module('@/components/ui/dialog', () => ({ Dialog: ({ open, children }) => open ? h('div', null, children) : null, DialogContent: passthrough, DialogDescription: passthrough, DialogFooter: passthrough, DialogHeader: passthrough, DialogTitle: passthrough }));
    const { default: Panel } = await import('./src/components/settings/ImportExportSettingsCard');
    const host = document.getElementById('root'); const root = createRoot(host);
    const button = text => [...host.querySelectorAll('button')].find(item => item.textContent === text);
    const click = async text => { assert.ok(button(text), text); await act(async () => button(text).click()); };
    const fill = async (selector, value) => { const input = host.querySelector(selector); assert.ok(input); const setter = Object.getOwnPropertyDescriptor(dom.window.HTMLInputElement.prototype, 'value').set; await act(async () => { setter.call(input, value); input.dispatchEvent(new dom.window.Event('input', { bubbles: true })); }); };
    await act(async () => root.render(h(Panel)));
    await click('Export');
    assert.match(host.textContent, /cannot be restored without it/);
    await fill('#settings-backup-password', 'correct-password');
    await fill('#settings-backup-confirmation', 'different-password');
    assert.ok(button('Continue').disabled);
    await fill('#settings-backup-confirmation', 'correct-password');
    await click('Continue'); assert.deepEqual(exported, ['correct-password']);
    selected = { fileName: 'old.json', json: '{"settingsVersion":2,"app":{},"ideal":{},"current":{},"policy":{}}', encrypted: false };
    await click('Import'); assert.ok(button('Replace settings')); assert.equal(imported.length, 0);
    await click('Replace settings'); assert.equal(imported.length, 1);
    selected = { fileName: 'new.json', json: 'ciphertext', encrypted: true };
    await click('Import'); await fill('#settings-backup-password', 'incorrect-password'); await click('Unlock');
    assert.match(errors.at(-1), /incorrect or backup damaged/); assert.equal(imported.length, 1);
    await fill('#settings-backup-password', 'correct-password'); await click('Unlock');
    assert.ok(button('Replace settings')); assert.equal(imported.length, 1);
    await click('Replace settings'); assert.equal(imported.length, 2);
    await act(async () => root.unmount()); dom.window.close();
    process.exit(0);
    } catch (error) { console.error(error); process.exit(1); }
  `;
  const process = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  let timedOut = false;
  const timer = setTimeout(() => { timedOut = true; process.kill(); }, 90_000);
  try {
    const stderr = await new Response(process.stderr).text();
    expect({ code: await process.exited, stderr, timedOut }).toEqual({ code: 0, stderr: "", timedOut: false });
  } finally { clearTimeout(timer); }
}, 105_000);
