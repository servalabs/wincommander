import { expect, test } from "bun:test";

declare const Bun: {
  which(name: string): string | null;
  spawn(args: string[], options: { stdout: "pipe"; stderr: "pipe" }): {
    stderr: ReadableStream<Uint8Array>; exited: Promise<number>; kill(): void;
  };
};

test("shows honest evidence warnings and does not duplicate them", async () => {
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
      let status = { continuityUnverified: true }, notices = [], listener;
      mock.module('./src/lib/evidence', () => ({ getEvidenceStatus: async () => {
        if (status === 'error') throw new Error('private path'); return status;
      }}));
      mock.module('./src/lib/notificationStore', () => ({ pushNotification: (...args) => notices.push(args) }));
      mock.module('@tauri-apps/api/event', () => ({ listen: async (_name, callback) => { listener = callback; return () => {}; } }));
      const { default: Bridge, updateEvidenceNotice } = await import('./src/components/EvidenceStatusBridge');
      const root = createRoot(document.getElementById('root'));
      await act(async () => { root.render(h(Bridge)); });
      assert.equal(notices.length, 1); assert.equal(notices[0][0], 'warn');
      assert.match(notices[0][1], /history may be missing/);
      assert.equal(notices[0][3], 'alert');
      listener(); assert.equal(notices.length, 2); assert.match(notices[1][1], /could not be recorded/);
      let last = updateEvidenceNotice('continuity', 'continuity', () => { throw new Error('duplicate'); });
      assert.equal(last, 'continuity');
      await act(async () => root.unmount());
      status = 'error'; notices = [];
      const second = createRoot(document.getElementById('root'));
      await act(async () => { second.render(h(Bridge)); });
      assert.equal(notices.length, 1); assert.match(notices[0][1], /could not be checked/);
      assert.doesNotMatch(notices[0][1], /private path/);
      await act(async () => second.unmount()); dom.window.close(); process.exit(0);
    } catch (error) { console.error(error); process.exit(1); }
  `;
  const process = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  const timer = setTimeout(() => process.kill(), 90_000);
  try {
    const stderr = await new Response(process.stderr).text();
    expect({ code: await process.exited, stderr }).toEqual({ code: 0, stderr: "" });
  } finally { clearTimeout(timer); }
}, 105_000);

test("evidence errors propagate and malformed status is rejected", async () => {
  const script = String.raw`
    import { mock } from 'bun:test';
    import { strict as assert } from 'node:assert';
    try {
      let response = new Error('ledger unavailable'), notices = [];
      mock.module('@tauri-apps/api/core', () => ({ invoke: async () => {
        if (response instanceof Error) throw response; return response;
      }}));
      mock.module('./src/lib/notificationStore', () => ({ pushNotification: (...args) => notices.push(args) }));
      const { readEvidence, clearEvidence, getEvidenceStatus, recordEvidence } = await import('./src/lib/evidence');
      await assert.rejects(readEvidence(), /ledger unavailable/);
      await assert.rejects(clearEvidence(), /ledger unavailable/);
      await assert.rejects(getEvidenceStatus(), /ledger unavailable/);
      recordEvidence('monitor', 'warn', 'test');
      await new Promise(resolve => setTimeout(resolve, 0));
      assert.equal(notices.length, 1); assert.match(notices[0][1], /could not be recorded/);
      assert.doesNotMatch(notices[0][1], /ledger unavailable/);
      response = { continuityUnverified: 'unknown' };
      await assert.rejects(getEvidenceStatus(), /Evidence status is unavailable/);
      response = { continuityUnverified: true };
      assert.deepEqual(await getEvidenceStatus(), response); process.exit(0);
    } catch (error) { console.error(error); process.exit(1); }
  `;
  const process = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  const timer = setTimeout(() => process.kill(), 90_000);
  try {
    const stderr = await new Response(process.stderr).text();
    expect({ code: await process.exited, stderr }).toEqual({ code: 0, stderr: "" });
  } finally { clearTimeout(timer); }
}, 105_000);
