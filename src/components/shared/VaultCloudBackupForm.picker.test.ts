// SPDX-License-Identifier: AGPL-3.0-or-later
import { expect, test } from "bun:test";

declare const Bun: {
  which(name: string): string | null;
  spawn(args: string[], options: { stdout: "pipe"; stderr: "pipe" }): {
    stderr: ReadableStream<Uint8Array>; exited: Promise<number>; kill(): void;
  };
};

test("backup picker adds selected Vault paths and rejects paths outside it", async () => {
  const script = String.raw`
    import { mock } from 'bun:test';
    import { strict as assert } from 'node:assert';
    import { JSDOM } from 'jsdom';
    const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost' });
    globalThis.window = dom.window; globalThis.document = dom.window.document;
    globalThis.navigator = dom.window.navigator; globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const React = await import('react'); const { act, createElement: h } = React;
    const { createRoot } = await import('react-dom/client');
    let chosen = ['X:\\Docs', 'X:\\Notes.txt']; const calls = [];
    mock.module('@tauri-apps/plugin-dialog', () => ({ open: async options => { calls.push(options); return chosen; } }));
    mock.module('@/components/ui/bp', () => ({ Button: ({ children, onClick, disabled }) => h('button', { onClick, disabled }, children) }));
    mock.module('@/components/ui/input', () => ({ Input: props => h('input', props) }));
    const { default: Form } = await import('./src/components/shared/VaultCloudBackupForm');
    const host = document.getElementById('root'); const root = createRoot(host);
    await act(async () => root.render(h(Form, { configuration: null, driveLabel: 'X:', disabled: false, canSave: true, onSave: () => {} })));
    const buttons = () => [...host.querySelectorAll('button')];
    const named = text => buttons().find(button => button.textContent === text);
    await act(async () => host.querySelectorAll('input[type=radio]')[1].click());
    await act(async () => named('Choose folders').click());
    assert.equal(calls[0].directory, true); assert.equal(calls[0].multiple, true); assert.equal(calls[0].defaultPath, 'X:\\');
    assert.equal(host.querySelector('textarea').value, 'Docs\nNotes.txt');
    chosen = ['Y:\\secret'];
    await act(async () => named('Choose files').click());
    assert.equal(calls[1].directory, false); assert.equal(host.querySelector('textarea').value, 'Docs\nNotes.txt');
    assert.match(host.querySelector('[role=alert]').textContent, /inside this Vault/);
    chosen = null;
    await act(async () => named('Choose files').click());
    assert.equal(host.querySelector('textarea').value, 'Docs\nNotes.txt');
    await act(async () => root.unmount()); dom.window.close();
  `;
  const child = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  const timer = setTimeout(() => child.kill(), 30_000);
  try {
    const stderr = await new Response(child.stderr).text();
    expect({ code: await child.exited, stderr }).toEqual({ code: 0, stderr: "" });
  } finally { clearTimeout(timer); }
}, 40_000);
