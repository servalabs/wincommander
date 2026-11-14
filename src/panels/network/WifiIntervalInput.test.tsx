import { expect, test } from 'bun:test';

declare const Bun: {
  which(name: string): string | null;
  spawn(args: string[], options: { stdout: 'pipe'; stderr: 'pipe' }): { stderr: ReadableStream<Uint8Array>; exited: Promise<number> };
};

test('Wi-Fi interval permits an intermediate value and saves only a valid completed value', async () => {
  const script = String.raw`
    import { strict as assert } from 'node:assert';
    import { JSDOM } from 'jsdom';
    const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost' });
    globalThis.window = dom.window; globalThis.document = dom.window.document;
    globalThis.navigator = dom.window.navigator; globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const React = await import('react'); const { act, createElement: h } = React;
    const { createRoot } = await import('react-dom/client');
    const { default: Input } = await import('./src/panels/network/WifiIntervalInput');
    const values = []; const host = document.getElementById('root'); const root = createRoot(host);
    await act(async () => root.render(h(Input, { label: 'Repeat alert (seconds)', value: 300, min: 30, max: 3600, disabled: false, onCommit: value => values.push(value) })));
    const input = host.querySelector('input');
    const setValue = Object.getOwnPropertyDescriptor(dom.window.HTMLInputElement.prototype, 'value').set;
    await act(async () => input.focus());
    await act(async () => { setValue.call(input, '3'); input.dispatchEvent(new dom.window.Event('input', { bubbles: true })); });
    assert.equal(input.value, '3'); assert.deepEqual(values, []);
    await act(async () => { setValue.call(input, '30'); input.dispatchEvent(new dom.window.Event('input', { bubbles: true })); });
    await act(async () => input.blur());
    assert.deepEqual(values, [30]);
    await act(async () => input.focus());
    await act(async () => { setValue.call(input, '2'); input.dispatchEvent(new dom.window.Event('input', { bubbles: true })); });
    await act(async () => input.blur());
    assert.deepEqual(values, [30]); assert.match(host.textContent, /Enter 30–3600 seconds/);
    await act(async () => root.unmount());
  `;
  const child = Bun.spawn([Bun.which('bun') ?? 'bun', '-e', script], { stdout: 'pipe', stderr: 'pipe' });
  const [status, stderr] = await Promise.all([child.exited, new Response(child.stderr).text()]);
  expect(stderr || 'no errors').toBe('no errors');
  expect(status).toBe(0);
});
