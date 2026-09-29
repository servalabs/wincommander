// Actual polling hook + refresh coordinator; simulated read-only inventory.
const assert = require('node:assert/strict');
const { chromium } = require(process.env.WINCOMMANDER_PLAYWRIGHT_MODULE || 'playwright');
const origin = new URL(process.argv[2] || 'http://127.0.0.1:1420');
if (!['127.0.0.1', 'localhost', '[::1]'].includes(origin.hostname)) throw Error('Loopback required');
const html = `<script type="module">import R from'/@react-refresh';R.injectIntoGlobalHook(window);window.$RefreshReg$=()=>{};window.$RefreshSig$=()=>type=>type;window.__vite_plugin_react_preamble_installed__=true;</script><div id="fixture"></div>`;
(async () => {
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage();
  page.setDefaultTimeout(10000);
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  try {
    const source = await (await page.request.get(new URL('/src/hooks/useActivePanelPoller.ts', origin).href)).text();
    const react = source.match(/from "([^"]*\/react\.js[^"]*)"/)?.[1]; assert.ok(react);
    const querySource = await (await page.request.get(new URL('/src/hooks/useVaultDriveLetters.ts', origin).href)).text();
    const query = querySource.match(/from "([^"]*@tanstack_react-query\.js[^"]*)"/)?.[1]; assert.ok(query);
    for (const [url, body] of [
      ['**/src/types/panels.ts*', "export const PANEL_MANIFESTS=[{id:'vault',refreshKey:'refreshVault'}];"],
      ['**/src/context/AppContext.tsx*', 'export const useAppState=()=>window.app;'],
      ['**/src/context/LiveMetricsContext.tsx*', 'export const useLiveMetrics=()=>window.metrics;'],
      ['**/src/context/AuthModeContext.tsx*', "export const useAuthMode=()=>({mode:window.authMode??'real',setMode:()=>{}});"],
      ['**/src/hooks/useBackend.ts*', 'export default()=>window.letterBackend;'],
    ]) await page.route(url, route => route.fulfill({ contentType: 'application/javascript', body }));
    await page.route('**/__vault_refresh__', route => route.fulfill({ contentType: 'text/html', body: html }));
    await page.goto(new URL('/__vault_refresh__', origin).href);
    await page.evaluate(async ({ reactUrl, queryUrl }) => {
      const React = (await import(reactUrl)).default;
      const ReactDOM = (await import('/node_modules/.vite/deps/react-dom_client.js')).default;
      const { QueryClient, QueryClientProvider } = await import(queryUrl);
      const { useActivePanelPoller } = await import('/src/hooks/useActivePanelPoller.ts');
      const { default: useVaultDriveLetters } = await import('/src/hooks/useVaultDriveLetters.ts');
      const { createVaultStatusRefresh, vaultInventoryFailureMessage } = await import('/src/lib/vaultStatusRefresh.ts');
      window.reads = []; window.letterReads = []; window.timers = new Map(); window.visible = true; window.authMode = 'real';
      let timerId = 0;
      window.setInterval = (fn, milliseconds) => { window.timers.set(++timerId, { fn, milliseconds }); return timerId; };
      window.clearInterval = id => window.timers.delete(id);
      Object.defineProperty(document, 'visibilityState', { get: () => window.visible ? 'visible' : 'hidden' });
      window.metrics = { refreshLiveMetrics: () => {} };
      window.letterBackend = { getAvailableDriveLetters: () => new Promise(resolve => window.letterReads.push({ resolve })) };
      const notify = () => window.rerender?.();
      window.app = { appSettings: { app: { modules: { vault: true } } }, encryptionStatus: null, vaultStatusError: null, manualBusy: false };
      const coordinator = createVaultStatusRefresh(value => { window.app.encryptionStatus = value; notify(); }, () => {}, error => {
        window.app.vaultStatusError = error ? vaultInventoryFailureMessage(error) : null; notify();
      }, busy => { window.app.manualBusy = busy; notify(); });
      window.app.refreshVault = (_silent, background = false) => coordinator(() => new Promise((resolve, reject) => window.reads.push({ resolve, reject })), background);
      function Fixture() {
        const [, rerender] = React.useState(0);
        const [state, setState] = React.useState({ activePanel: 'vault', paused: false });
        const driveLetters = useVaultDriveLetters(true);
        const subscribedLetters = useVaultDriveLetters(false);
        window.driveLetters = driveLetters; window.subscribedLetters = subscribedLetters;
        window.setPanel = setState; window.rerender = () => rerender(n => n + 1);
        useActivePanelPoller(state);
        return React.createElement(React.Fragment, null,
          React.createElement('button', { disabled: window.app.manualBusy, onClick: () => window.app.refreshVault(false) }, 'Force refresh'),
          React.createElement('button', { disabled: driveLetters.loading, onClick: () => driveLetters.refresh() }, 'Refresh letters'));
      }
      const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      ReactDOM.createRoot(document.getElementById('fixture')).render(React.createElement(QueryClientProvider, { client: queryClient }, React.createElement(Fixture)));
    }, { reactUrl: react, queryUrl: query });
    const settle = async (index, value) => { await page.evaluate(({ index, value }) => window.reads[index].resolve(value), { index, value }); await page.evaluate(() => new Promise(resolve => requestAnimationFrame(resolve))); };
    const settleLetters = async (index, letters) => { await page.evaluate(({ index, letters }) => window.letterReads[index].resolve({ success: true, data: { letters } }), { index, letters }); await page.evaluate(() => new Promise(resolve => requestAnimationFrame(resolve))); };
    const tick = milliseconds => page.evaluate(milliseconds => { for (const timer of window.timers.values()) if (timer.milliseconds === milliseconds) timer.fn(); }, milliseconds);
    await page.waitForFunction(() => window.reads.length === 1);
    await page.waitForFunction(() => window.letterReads.length === 1);
    assert.deepEqual(await page.evaluate(() => [...window.timers.values()].map(timer => timer.milliseconds).sort((a,b)=>a-b)), [5000,30000]);
    await settleLetters(0, ['J','Z']);
    assert.deepEqual(await page.evaluate(() => window.subscribedLetters.letters), ['J','Z'], 'A consumer reads the startup result without issuing another request');
    await settle(0, ['J:']);
    await page.evaluate(() => { window.dispatchEvent(new Event('focus')); document.dispatchEvent(new Event('visibilitychange')); document.body.click(); });
    assert.equal(await page.evaluate(() => window.reads.length), 1, 'Focus/click is not a refresh command');
    assert.equal(await page.evaluate(() => window.letterReads.length), 1, 'Focus/click does not refetch cached drive letters');
    await page.evaluate(() => window.setPanel({ activePanel: 'apps', paused: false }));
    await page.waitForFunction(() => [...window.timers.values()].every(timer => timer.milliseconds !== 5000));
    assert.deepEqual(await page.evaluate(() => [...window.timers.values()].map(timer => timer.milliseconds)), [30000], 'The app-owned drive cache continues outside the Secure Storage panel');
    await page.evaluate(() => window.setPanel({ activePanel: 'vault', paused: false }));
    await page.waitForFunction(() => [...window.timers.values()].some(timer => timer.milliseconds === 5000));
    assert.equal(await page.evaluate(() => window.reads.length), 1, 'Re-entering a loaded panel does not restart observation');
    await tick(5000); await page.waitForFunction(() => window.reads.length === 2);
    const button = page.getByRole('button', { name: 'Force refresh' });
    assert.equal(await button.isEnabled(), true, 'Background work cannot block an explicit fresh probe');
    await button.click(); await page.waitForFunction(() => window.reads.length === 3);
    assert.equal(await button.isDisabled(), true);
    await tick(5000); assert.equal(await page.evaluate(() => window.reads.length), 3);
    await settle(2, ['K:']);
    await page.evaluate(() => window.reads[1].reject(Error('old unrelated failure')));
    await page.waitForFunction(() => window.app.encryptionStatus?.[0] === 'K:');
    assert.equal(await page.evaluate(() => window.app.vaultStatusError), null);
    await tick(5000); await page.waitForFunction(() => window.reads.length === 4);
    await page.evaluate(() => window.reads[3].reject(Error('caller_root_unavailable C:\\private')));
    await page.waitForFunction(() => Boolean(window.app.vaultStatusError));
    assert.deepEqual(await page.evaluate(() => window.app.encryptionStatus), ['K:'], 'Failed read retains only explicitly stale observation');
    assert.equal(await page.evaluate(() => window.app.vaultStatusError.includes('private')), false);
    await button.click(); await page.waitForFunction(() => window.reads.length === 5);
    await settle(4, []);
    assert.deepEqual(await page.evaluate(() => window.app.encryptionStatus), [], 'Confirmed removal clears old rows');
    assert.equal(await page.evaluate(() => window.app.vaultStatusError), null);
    const letterButton = page.getByRole('button', { name: 'Refresh letters' });
    await letterButton.click(); await page.waitForFunction(() => window.letterReads.length === 2);
    await settleLetters(1, ['K']);
    assert.deepEqual(await page.evaluate(() => window.subscribedLetters.letters), ['K'], 'Manual refresh updates every cache subscriber');
    await tick(30000); await page.waitForFunction(() => window.letterReads.length === 3);
    await settleLetters(2, ['L']);
    assert.deepEqual(await page.evaluate(() => window.subscribedLetters.letters), ['L'], 'The startup owner refreshes drive letters on its bounded interval');
    await page.evaluate(() => { window.visible = false; }); await tick(5000);
    assert.equal(await page.evaluate(() => window.reads.length), 5);
    await page.evaluate(() => window.setPanel({ activePanel: 'vault', paused: true }));
    await page.waitForFunction(() => [...window.timers.values()].every(timer => timer.milliseconds !== 5000));
    const realLetterReadCount = await page.evaluate(() => window.letterReads.length);
    await page.evaluate(() => { window.authMode = 'decoy'; window.rerender(); });
    await page.waitForFunction(() => window.subscribedLetters.unavailable === true && window.subscribedLetters.letters.length === 0);
    assert.equal(await page.evaluate(() => window.subscribedLetters.refresh()), null, 'Decoy mode refuses an explicit drive-letter refresh');
    assert.equal(await page.evaluate(() => window.letterReads.length), realLetterReadCount, 'Decoy mode never reads real drive availability');
    await page.waitForFunction(() => window.timers.size === 0);
    assert.deepEqual(errors, []);
    console.log('PASS: inventory initial+timed reads; drive-letter startup preload, shared consumer cache, no focus refetch, manual refresh, bounded interval and decoy refusal; manual inventory bypasses background, latest verification wins, stale errors stay truthful, confirmed removal clears rows, hidden/paused timers skip.');
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
