import { expect, test } from "bun:test";

declare const Bun: { which(name: string): string | null; spawn(args: string[], options: { stdout: "pipe"; stderr: "pipe" }): { stderr: ReadableStream<Uint8Array>; exited: Promise<number> } };

test("lockdown phrase can be added inline and cancelled", async () => {
  const script = String.raw`
    import { mock } from 'bun:test';
    import { strict as assert } from 'node:assert';
    import { JSDOM } from 'jsdom';
    const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost' });
    globalThis.window = dom.window; globalThis.document = dom.window.document;
    globalThis.navigator = dom.window.navigator; globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const React = await import('react'); const { act, createElement: h } = React;
    const { createRoot } = await import('react-dom/client');
    mock.module('@tauri-apps/api/core', () => ({ invoke: async () => false }));
    mock.module('@/components/ui/bp', () => ({ Button: ({ children, onClick, disabled }) => h('button', { onClick, disabled }, children), Icon: () => null }));
    mock.module('./src/panels/privacy/MonitorIntros', () => ({ PanicKeywordIntro: () => null }));
    const { default: Panel } = await import('./src/panels/privacy/LockdownWordsSection');
    const host = document.getElementById('root'); const root = createRoot(host);
    await act(async () => root.render(h(Panel, { isAdvanced: true, enabled: true, phrases: [], onPatch() {}, expanded: true, onExpandedChange() {}, bare: true })));
    const help = host.querySelector('[aria-label="How typed lockdown words work"]');
    assert.ok(help); assert.equal(help.textContent, '?');
    const button = [...host.querySelectorAll('button')].find(x => x.textContent.includes('Add phrase'));
    assert.ok(button); assert.ok(button.classList.contains('lockdown-trigger-action--add')); await act(async () => button.click());
    assert.ok(host.querySelector('[aria-label="Lockdown secret phrase"]'));
    assert.ok(host.querySelector('.lockdown-trigger-form'));
    assert.equal(host.querySelector('[role="dialog"]'), null);
    const cancel = [...host.querySelectorAll('button')].find(x => x.textContent === 'Cancel');
    assert.ok(cancel); await act(async () => cancel.click());
    assert.equal(host.querySelector('[aria-label="Lockdown secret phrase"]'), null);
    await act(async () => root.unmount());
  `;
  const process = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  const stderr = await new Response(process.stderr).text();
  expect({ code: await process.exited, stderr }).toEqual({ code: 0, stderr: "" });
});
