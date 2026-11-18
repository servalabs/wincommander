import { expect, test } from "bun:test";

declare const Bun: {
  which(name: string): string | null;
  spawn(args: string[], options: { stdout: "pipe"; stderr: "pipe" }): { stderr: ReadableStream<Uint8Array>; exited: Promise<number> };
};

test("file watch folder picker fills the directory and preserves it when cancelled", async () => {
  const script = String.raw`
    import { mock } from 'bun:test';
    import { strict as assert } from 'node:assert';
    import { JSDOM } from 'jsdom';
    const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost' });
    globalThis.window = dom.window; globalThis.document = dom.window.document;
    globalThis.navigator = dom.window.navigator; globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const React = await import('react'); const { act, createElement: h } = React;
    const { createRoot } = await import('react-dom/client');
    let result = 'C:\\Users\\Parth\\Documents'; let options;
    mock.module('@tauri-apps/plugin-dialog', () => ({ open: async (value) => { options = value; return result; } }));
    mock.module('./src/hooks/armBackend', () => ({ armBackend: async () => true }));
    mock.module('./src/components/ui/icon', () => ({ Icon: () => null }));
    const { default: Panel } = await import('./src/panels/privacy/FileWatchTriggerSection');
    const host = document.getElementById('root'); const root = createRoot(host);
    await act(async () => root.render(h(Panel, { settings: { enabled: false, rules: [] }, onPatch: () => {}, bare: true })));
    await act(async () => [...host.querySelectorAll('button')].find(b => b.textContent.includes('Add rule')).click());
    const choose = host.querySelector('[aria-label="Choose directory to watch"]');
    await act(async () => choose.click());
    assert.equal(options.directory, true); assert.equal(options.multiple, false);
    assert.equal(host.querySelector('[aria-label="Directory path"]').value, result);
    result = null;
    await act(async () => choose.click());
    assert.equal(host.querySelector('[aria-label="Directory path"]').value, 'C:\\Users\\Parth\\Documents');
    await act(async () => root.unmount());
  `;
  const process = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  const stderr = await new Response(process.stderr).text();
  expect({ code: await process.exited, stderr }).toEqual({ code: 0, stderr: "" });
});
