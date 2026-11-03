import { expect, test } from "bun:test";

declare const Bun: {
  which(name: string): string | null;
  spawn(args: string[], options: { stdout: "pipe"; stderr: "pipe" }): {
    stderr: ReadableStream<Uint8Array>; exited: Promise<number>; kill(): void;
  };
};

test("inactivity card presents watchdog state, launch reset and unknown status honestly", async () => {
  const script = String.raw`
    import { mock } from 'bun:test';
    import { strict as assert } from 'node:assert';
    import { JSDOM } from 'jsdom';
    const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost' });
    globalThis.window = dom.window; globalThis.document = dom.window.document;
    globalThis.navigator = dom.window.navigator; globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const React = await import('react'); const { act, createElement: h } = React;
    const { createRoot } = await import('react-dom/client');
    let status = { config: { enabled: true, thresholdDays: 14, flowIdToFire: null, lastActivityAt: '2026-01-01T00:00:00Z', lastFiredAt: null }, state: 'lockdown_disabled', startupResetFailed: true };
    let fail = false;
    mock.module('@tauri-apps/api/core', () => ({ invoke: async (name, args) => {
      if (name === 'get_dead_mans_switch_status') { if (fail) throw new Error('offline'); return status; }
      if (name === 'set_dead_mans_switch_config') { status = { ...status, config: args.config, state: 'disabled' }; return args.config; }
      if (name === 'reset_dead_mans_switch_timer') return status.config;
      throw new Error('unexpected ' + name);
    }}));
    mock.module('@/components/ui/bp', () => ({ Button: ({ children, onClick, disabled, style }) => h('button', { onClick, disabled, style }, children), Icon: () => null }));
    mock.module('./src/components/shared/SectionCard', () => ({ default: ({ children }) => h('section', null, children) }));
    const { default: Panel } = await import('./src/panels/privacy/CheckInTimerSection');
    const host = document.getElementById('root'); let root = createRoot(host);
    await act(async () => { root.render(h(Panel, { bare: true })); });
    assert.match(host.textContent, /Lockdown off/);
    assert.match(host.textContent, /timer can still record a trigger/);
    assert.match(host.textContent, /including automatic startup/);
    assert.match(host.textContent, /watchdog may still run/);
    assert.doesNotMatch(host.textContent, /Active/);
    const toggle = host.querySelector('[role="switch"]');
    await act(async () => { toggle.click(); });
    assert.match(host.textContent, /inactivity timer is off/);
    fail = true;
    const reset = [...host.querySelectorAll('button')].find(button => button.textContent === "I'm alive");
    await act(async () => { reset.click(); });
    assert.match(host.textContent, /status could not be confirmed/);
    assert.ok(host.querySelector('[role="alert"]'));
    await act(async () => { root.unmount(); });
    fail = false;
    const valid = status;
    status = { state: 'monitoring', config: null, startupResetFailed: false };
    root = createRoot(host);
    await act(async () => { root.render(h(Panel, { bare: true })); });
    assert.match(host.textContent, /status could not be confirmed/);
    assert.doesNotMatch(host.textContent, /Loading inactivity timer/);
    await act(async () => { root.unmount(); });
    status = valid;
    status = { ...status, config: { ...status.config, lastFiredAt: '2026-01-02T00:00:00Z' }, state: 'triggered', startupResetFailed: false };
    root = createRoot(host);
    await act(async () => { root.render(h(Panel, { bare: true })); });
    assert.match(host.textContent, /Trigger recorded/);
    assert.match(host.textContent, /does not confirm that Lockdown completed/);
    await act(async () => { root.unmount(); }); dom.window.close();
  `;
  const process = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  let timedOut = false;
  const timer = setTimeout(() => { timedOut = true; process.kill(); }, 30_000);
  try {
    const stderr = await new Response(process.stderr).text();
    expect({ code: await process.exited, stderr, timedOut }).toEqual({ code: 0, stderr: "", timedOut: false });
  } finally { clearTimeout(timer); }
}, 45_000);
