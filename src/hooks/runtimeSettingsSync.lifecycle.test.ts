import { expect, test } from "bun:test";

declare const Bun: {
  which(name: string): string | null;
  spawn(args: string[], options: { stdout: "pipe"; stderr: "pipe" }): {
    stderr: ReadableStream<Uint8Array>;
    exited: Promise<number>;
  };
};

test("runtime sync ignores unchanged settings identities and preserves actual edits and decoy state", async () => {
  const script = `
    import { mock } from 'bun:test';
    import { strict as assert } from 'node:assert';
    const react = await import('react');
    const slots = [];
    let cursor = 0;
    let effects = [];
    const equal = (a, b) => a && b && a.length === b.length && a.every((v, i) => Object.is(v, b[i]));
    mock.module('react', () => ({ ...react,
      useEffect: (effect, deps) => {
        const i = cursor++;
        if (!equal(slots[i], deps)) { slots[i] = deps; effects.push(effect); }
      },
      useMemo: (create, deps) => {
        const i = cursor++;
        if (!equal(slots[i]?.deps, deps)) slots[i] = { deps, value: create() };
        return slots[i].value;
      },
    }));
    const calls = [];
    let mode = 'real';
    mock.module('@tauri-apps/api/core', () => ({ invoke: async (command, args) => { calls.push({ command, args }); } }));
    mock.module('./src/context/AuthModeContext', () => ({ useAuthMode: () => ({ mode }) }));
    mock.module('./src/utils/toast', () => ({ showError: () => {} }));
    mock.module('./src/lib/diagnostics', () => ({ newDiagnosticOperationId: () => 'test', recordDiagnostic: () => {} }));
    const { default: usePasteMonitor, resolveCategories } = await import('./src/hooks/usePasteMonitor');
    const { default: useLockdownWords } = await import('./src/hooks/useLockdownWords');
    const count = command => calls.filter(call => call.command === command).length;
    const render = (categories, phrases = [], paid = true) => {
      cursor = 0; effects = [];
      usePasteMonitor(false, resolveCategories(categories), true, false, 30, false);
      useLockdownWords(false, phrases, paid);
      effects.forEach(effect => effect());
    };
    render(undefined);
    for (let i = 0; i < 20; i++) render(undefined, []);
    assert.equal(count('set_paste_monitor_categories'), 1, 'new equivalent category objects must not resync');
    assert.equal(count('set_lockdown_words'), 1, 'new empty phrase arrays must not resync');
    render({ cloudApi: false }, [{ hash: 'hash-a', label: 'old label', mode: 'decoy' }]);
    assert.equal(count('set_paste_monitor_categories'), 2);
    assert.equal(count('set_lockdown_words'), 2);
    render({ cloudApi: false }, [{ hash: 'hash-a', label: 'new label', mode: 'decoy' }]);
    assert.equal(count('set_paste_monitor_categories'), 2);
    assert.equal(count('set_lockdown_words'), 3, 'metadata edits with unchanged hashes still reach native');
    mode = 'decoy';
    render({ cloudApi: false });
    assert.equal(count('set_lockdown_words'), 3, 'decoy must not clear registered phrases');
    mode = 'real';
    render({ cloudApi: false });
    assert.equal(count('set_lockdown_words'), 4, 'leaving decoy resyncs authoritative state');
    render({ cloudApi: false }, [], false);
    assert.equal(count('set_lockdown_words'), 4, 'unpaid sessions must not sync phrases');
  `;
  const process = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  const stderr = await new Response(process.stderr).text();
  expect({ code: await process.exited, stderr }).toEqual({ code: 0, stderr: "" });
});
