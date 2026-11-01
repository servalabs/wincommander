import { expect, test } from "bun:test";

declare const Bun: {
  which(name: string): string | null;
  spawn(args: string[], options: { stdout: "pipe"; stderr: "pipe" }): {
    stderr: ReadableStream<Uint8Array>;
    exited: Promise<number>;
  };
};

test("cleanup viewers confirm actions and refresh actual browser results", async () => {
  const script = `
    import { mock } from 'bun:test';
    import { strict as assert } from 'node:assert';
    const react = await import('react');
    let slots = [], cursor = 0;
    mock.module('react', () => ({ ...react,
      useState: initial => {
        const i = cursor++;
        if (!(i in slots)) slots[i] = initial;
        return [slots[i], value => { slots[i] = typeof value === 'function' ? value(slots[i]) : value; }];
      },
      useRef: initial => { const i = cursor++; if (!(i in slots)) slots[i] = { current: initial }; return slots[i]; },
      useEffect: () => {},
    }));
    mock.module('./src/components/ui/bp', () => ({ Dialog: 'Dialog', Button: 'Button', Icon: 'Icon' }));
    mock.module('./src/components/shared/AnimatedList', () => ({ staggerDelay: () => 0 }));
    mock.module('framer-motion', () => ({ motion: { tr: 'tr' }, AnimatePresence: 'AnimatePresence' }));
    const calls = [], errors = [], successes = [], caches = [];
    let confirmed = false, confirmWait = null, response = { success: true, data: { stdout: 'ok: cleared 1/2 (1 locked, likely still running; legacy=0)' } };
    const row = { browser: 'Chrome', profilePath: 'C:/fixture', totalSizeKB: 1, artifacts: [{name:'History',sizeKB:1}] };
    let audit = { success: true, data: { browsers: [row] } };
    mock.module('./src/hooks/useBackend', () => ({ default: () => ({
      getWlanProfiles: async () => ({ success: true, data: { profiles: [{ name: 'fixture' }] } }),
      removeWlanProfile: async (...args) => { calls.push(['wlan', ...args]); return {success:true}; },
      getBrowserFootprints: async () => { calls.push(['audit']); return audit; },
      clearBrowserFootprints: async (...args) => { calls.push(['clear', ...args]); return response; },
    }) }));
    mock.module('./src/utils/toast', () => ({ showError: text => errors.push(text), showSuccess: text => successes.push(text) }));
    mock.module('./src/panels/cleanup/useCleanupScan', () => ({ updateCacheEntry: (...args) => caches.push(args) }));
    mock.module('./src/components/shared/AppConfirmDialog', () => ({ useAppConfirm: () => async () => confirmWait ? await confirmWait : confirmed }));
    const { useCleanupLegacyDialogs } = await import('./src/panels/cleanup/useCleanupLegacyDialogs');
    const render = (map = {}) => { cursor = 0; return useCleanupLegacyDialogs(map); };
    const find = (el, pred) => {
      if (!el || typeof el !== 'object') return;
      if (pred(el)) return el;
      for (const child of react.Children.toArray(el.props?.children)) { const hit = find(child, pred); if (hit) return hit; }
    };
    let ui = render(); await ui.openers.handleWlanProfiles(); ui = render();
    let button = find(ui.dialogs, e => e.props?.['aria-label'] === 'Forget Wi-Fi profile fixture');
    assert.ok(button); await button.props.onClick(); assert.equal(calls.length, 0);
    let release; confirmWait = new Promise(resolve => { release = resolve; });
    const pending = button.props.onClick(); await button.props.onClick(); assert.equal(calls.length, 0);
    release(true); await pending; confirmWait = null;
    assert.deepEqual(calls.shift(), ['wlan', 'fixture', true]);
    assert.equal(caches.at(-1)[1].raw.profiles.length, 1);
    assert.match(errors.at(-1), /still present/);
    ui = render(); await ui.openers.handleBrowserFootprints(); ui = render(); calls.length = 0;
    button = find(ui.dialogs, e => e.props?.text === 'CLEAR THIS BROWSER'); assert.ok(button);
    confirmed = false; await button.props.onClick(); assert.equal(calls.length, 0);
    confirmed = true; await button.props.onClick();
    assert.deepEqual(calls, [['clear','Chrome','C:/fixture',true],['audit']]);
    assert.equal(caches.at(-1)[1].raw.browsers.length, 1);
    assert.match(errors.at(-1), /locked/); assert.equal(successes.length, 0);
    ui = render(); const all = find(ui.dialogs, e => e.props?.text === 'CLEAR ALL BROWSERS');
    confirmed = false; calls.length = 0; await all.props.onClick(); assert.equal(calls.length, 0);
    confirmed = true; audit = {success:false,error:'unavailable'}; response = {success:true,data:{stdout:'ok: cleared 0 (legacy=0)'}};
    const previous = caches.length; await all.props.onClick();
    assert.deepEqual(calls,[['clear',undefined,undefined,true],['audit']]); assert.equal(caches.length,previous + 1); assert.equal(caches.at(-1)[1].raw, undefined); assert.match(caches.at(-1)[1].error, /refreshed/);
    assert.match(errors.at(-1), /could not be refreshed/); assert.equal(successes.length,0);
    ui = render({browserFootprints:{raw:{browsers:[row]}}});
    const beforeReopen = calls.filter(c => c[0] === 'audit').length;
    await ui.openers.handleBrowserFootprints();
    assert.equal(calls.filter(c => c[0] === 'audit').length, beforeReopen + 1);
    audit = {success:true,data:{browsers:[row]}};
    ui = render({browserFootprints:{raw:{browsers:[row]}}});
    const retry = find(ui.dialogs, e => e.props?.text === 'RETRY AUDIT'); assert.ok(retry);
    const auditsBefore = calls.filter(c => c[0] === 'audit').length;
    retry.props.onClick(); for (let i=0;i<8;i++) await Promise.resolve();
    assert.equal(calls.filter(c => c[0] === 'audit').length, auditsBefore + 1);
    audit = {success:true,data:{browsers:[]}}; response = {success:false,error:'all locked'};
    await all.props.onClick(); assert.equal(caches.at(-1)[1].raw.browsers.length,0);
    assert.equal(errors.at(-1),'all locked'); assert.equal(successes.length,0);
    audit = {success:true,data:{browsers:[{...row,browser:'',profilePath:''}]}};
    ui = render(); await ui.openers.handleBrowserFootprints(); ui = render();
    calls.length = 0; button = find(ui.dialogs, e => e.props?.text === 'CLEAR THIS BROWSER');
    await button.props.onClick(); assert.equal(calls.length, 0); assert.match(errors.at(-1), /identity is incomplete/);
    audit = {success:true,data:{browsers:{invalid:true}}};
    ui = render(); await ui.openers.handleBrowserFootprints(true);
    assert.equal(caches.at(-1)[1].raw, undefined); assert.match(errors.at(-1), /Failed to audit/);
  `;
  const proc = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
  const stderr = await new Response(proc.stderr).text();
  expect({ code: await proc.exited, stderr }).toEqual({ code: 0, stderr: "" });
});
