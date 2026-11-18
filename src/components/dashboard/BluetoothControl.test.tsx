import { expect, test } from "bun:test";

declare const Bun: {
  which(name: string): string | null;
  spawn(args: string[], options: { stdout: "pipe"; stderr: "pipe" }): {
    stderr: ReadableStream<Uint8Array>; exited: Promise<number>; kill(): void;
  };
};

test("Bluetooth retries failed verification silently and restores the confirmed control", async () => {
  const script = String.raw`
    import { mock } from 'bun:test';
    import { strict as assert } from 'node:assert';
    import { JSDOM } from 'jsdom';

    const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost', pretendToBeVisual: true });
    globalThis.window = dom.window;
    globalThis.document = dom.window.document;
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    let poll;
    window.setInterval = (callback) => { poll = callback; return 1; };
    const { act, createElement } = await import('react');
    const { createRoot } = await import('react-dom/client');
    const allowed = { schemaVersion: 2, blocked: false, state: 'allowed', canChange: true, message: '' };
    const reads = [];
    const commands = [];
    const confirmations = [];
    const notifications = [];
    mock.module('@tauri-apps/api/core', () => ({
      invoke: (command) => {
        commands.push(command);
        if (command !== 'bluetooth_control_status') throw new Error('Unexpected change request');
        return new Promise((resolve, reject) => reads.push({ resolve, reject }));
      },
    }));
    mock.module('./src/context/AppContext', () => ({ useAppState: () => ({ systemInfo: { isAdmin: true } }) }));
    mock.module('./src/components/shared/AppConfirmDialog', () => ({
      useAppConfirm: () => async () => { confirmations.push(true); return false; },
    }));
    mock.module('./src/utils/toast', () => ({
      showWarning: async (message) => notifications.push(message),
      showError: async (message) => notifications.push(message),
    }));
    const { default: BluetoothControl } = await import('./src/components/dashboard/BluetoothControl');
    const host = document.getElementById('root');
    const button = () => host.querySelector('button');
    const assertQuiet = () => {
      assert.equal(button().textContent, 'BT');
      assert.equal(button().getAttribute('aria-label'), 'Bluetooth');
      assert.equal(button().getAttribute('aria-busy'), 'false');
      assert.equal(button().hasAttribute('title'), false);
      assert.equal(button().disabled, true);
      assert.deepEqual(notifications, []);
    };
    const root = createRoot(host);
    try {
      await act(async () => root.render(createElement(BluetoothControl)));
      assertQuiet();
      await act(async () => reads.shift().resolve(allowed));
      assert.equal(button().getAttribute('aria-label'), 'Bluetooth ON');
      assert.equal(button().disabled, false);

      await act(async () => poll());
      await act(async () => reads.shift().reject(new Error('service offline')));
      assertQuiet();
      await act(async () => button().click());
      assert.equal(commands.length, 2);

      await act(async () => poll());
      assertQuiet();
      await act(async () => reads.shift().resolve({ ...allowed, blocked: true, state: 'blocked' }));
      assert.equal(button().getAttribute('aria-label'), 'Bluetooth OFF');
      assert.equal(button().disabled, false);
      assert.deepEqual(commands, Array(3).fill('bluetooth_control_status'));
      assert.deepEqual(confirmations, []);
      assert.deepEqual(notifications, []);
    } finally {
      await act(async () => root.unmount());
      dom.window.close();
    }
  `;
  const child = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  const timer = setTimeout(() => child.kill(), 90_000);
  try {
    const stderr = await new Response(child.stderr).text();
    expect({ code: await child.exited, stderr }).toEqual({ code: 0, stderr: "" });
  } finally { clearTimeout(timer); }
}, 105_000);
