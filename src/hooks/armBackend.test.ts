import { expect, test } from "bun:test";

declare const Bun: {
  which(name: string): string | null;
  spawn(args: string[], options: { stdout: "pipe"; stderr: "pipe" }): { stderr: ReadableStream<Uint8Array>; exited: Promise<number> };
  file(path: string): { text(): Promise<string> };
};

async function run(script: string) {
  const process = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  const stderr = await new Response(process.stderr).text();
  expect({ code: await process.exited, stderr }).toEqual({ code: 0, stderr: "" });
}

test("armBackend reports false, shows an error and logs when the arming invoke fails", async () => {
  await run(String.raw`
    import { mock } from 'bun:test';
    import { strict as assert } from 'node:assert';
    const errors = [], logs = [], successes = [];
    let fail = true;
    mock.module('@tauri-apps/api/core', () => ({ invoke: async (name) => {
      if (name === 'write_log_record') return; if (fail) throw new Error('boom'); } }));
    mock.module('./src/utils/toast', () => ({ showError: (m) => errors.push(m), showSuccess: (m) => successes.push(m) }));
    mock.module('./src/lib/logger', () => ({ logRecord: async (level, m) => logs.push([level, m]) }));
    const { armBackend } = await import('./src/hooks/armBackend');
    assert.equal(await armBackend('set_capture_protection', { enabled: true }, 'Capture protection is NOT active'), false);
    assert.equal(errors.length, 1); assert.match(errors[0], /NOT active.*boom/);
    assert.equal(logs.length, 1); assert.equal(logs[0][0], 'error'); assert.match(logs[0][1], /set_capture_protection failed/);
    fail = false;
    assert.equal(await armBackend('set_capture_protection', { enabled: true }, 'x'), true);
    assert.equal(errors.length, 1);
  `);
});

test("AuthModeContext surfaces a failed decoy backstop instead of swallowing it", async () => {
  await run(String.raw`
    import { mock } from 'bun:test';
    import { strict as assert } from 'node:assert';
    import { JSDOM } from 'jsdom';
    const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost' });
    globalThis.window = dom.window; globalThis.document = dom.window.document;
    globalThis.navigator = dom.window.navigator; globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const React = await import('react'); const { act, createElement: h } = React;
    const { createRoot } = await import('react-dom/client');
    const errors = [], logs = [];
    mock.module('@tauri-apps/api/core', () => ({ invoke: async () => { throw new Error('backstop offline'); } }));
    mock.module('./src/utils/toast', () => ({ showError: (m) => errors.push(m) }));
    mock.module('./src/lib/logger', () => ({ logRecord: async (l, m) => logs.push([l, m]) }));
    const { AuthModeProvider, useAuthMode } = await import('./src/context/AuthModeContext');
    let api; const Probe = () => { api = useAuthMode(); return h('span', null, api.mode); };
    const host = document.getElementById('root'); const root = createRoot(host);
    await act(async () => { root.render(h(AuthModeProvider, null, h(Probe))); });
    await act(async () => { api.setMode('decoy'); });
    assert.equal(errors.length, 1); assert.match(errors[0], /NOT write-protected.*backstop offline/);
    assert.equal(logs.length, 1); assert.match(logs[0][1], /set_decoy_mode failed/);
    assert.equal(host.textContent, 'decoy'); // fail closed: decoy view is kept, but the user is told
    await act(async () => root.unmount());
  `);
});

test("file watch toggle is not persisted ON and shows 'not active' when the watcher fails to start", async () => {
  await run(String.raw`
    import { mock } from 'bun:test';
    import { strict as assert } from 'node:assert';
    import { JSDOM } from 'jsdom';
    const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost' });
    globalThis.window = dom.window; globalThis.document = dom.window.document;
    globalThis.navigator = dom.window.navigator; globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const React = await import('react'); const { act, createElement: h } = React;
    const { createRoot } = await import('react-dom/client');
    const errors = [], patches = [];
    mock.module('@tauri-apps/api/core', () => ({ invoke: async (name) => { if (name === 'start_file_watch_triggers') throw new Error('access denied'); } }));
    mock.module('./src/utils/toast', () => ({ showError: (m) => errors.push(m) }));
    mock.module('./src/lib/logger', () => ({ logRecord: async () => {} }));
    mock.module('@/components/ui/bp', () => ({ Button: ({ children, onClick }) => h('button', { onClick }, children), Icon: () => null }));
    mock.module('./src/components/ui/icon', () => ({ Icon: () => null }));
    const { default: Panel } = await import('./src/panels/privacy/FileWatchTriggerSection');
    const rules = [{ id: 'a', path: 'C:/x', namePattern: '*.key', event: 'created', enabled: true }];
    const host = document.getElementById('root'); const root = createRoot(host);
    await act(async () => { root.render(h(Panel, { settings: { enabled: false, rules }, onPatch: (p) => patches.push(p), bare: true })); });
    const toggle = host.querySelector('[role="switch"]');
    await act(async () => { toggle.click(); });
    assert.equal(patches.filter((p) => p.enabled === true).length, 0);
    assert.equal(errors.length, 1); assert.match(errors[0], /NOT active.*access denied/);
    assert.match(host.textContent, /not active/);
    await act(async () => root.unmount());
  `);
});

test("arming call sites no longer swallow failures", async () => {
  const read = async (path: string) => await Bun.file(path).text();
  const checks: Array<[string, RegExp[]]> = [
    ["src/panels/secret/index.tsx", [/enter_calculator_mode"\)\.catch/, /update_hide_hotkey"[^)]*\)\.catch\(\(\) => \{\}\)/, /update_autostart_task_identity"[^)]*\}\)\.catch\(\(\) => \{\}\)/]],
    ["src/panels/privacy/FileWatchTriggerSection.tsx", [/start_file_watch_triggers[^\n]*catch\(\(\) => \{\}\)/]],
    ["src/context/AuthModeContext.tsx", [/set_decoy_mode[^\n]*catch\(\(\) => \{\}\)/]],
    ["src/App.tsx", [/set_capture_protection[^\n]*catch\(\(\) => \{\}\)/]],
    ["src/panels/dashboard/index.tsx", [/lock_to_calculator"\)\.catch\(\(\) => \{\}\)/]],
    ["src/panels/privacy/PanicHotkeyTrigger.tsx", [/update_panic_hotkey[^\n]*catch/]],
  ];
  for (const [path, patterns] of checks) {
    const text = await read(path);
    for (const pattern of patterns) expect({ path, hit: pattern.test(text) }).toEqual({ path, hit: false });
  }
  const secret = await read("src/panels/secret/index.tsx");
  // A failed enter_calculator_mode must roll back the persisted lock setting and never reach showSuccess.
  const catchAt = secret.indexOf('reportArmFailure("enter_calculator_mode"');
  expect(catchAt).toBeGreaterThan(-1);
  const after = secret.slice(catchAt, secret.indexOf("showSuccess(next", catchAt));
  expect(after).toContain('set_startup_pin_enabled", { enabled: false }');
  expect(after).toContain("return;");
});
