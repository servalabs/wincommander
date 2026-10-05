// Real preference component with browser-local settings; no native writes.
const assert = require('node:assert/strict');
const { chromium } = require(process.env.WINCOMMANDER_PLAYWRIGHT_MODULE || 'playwright');
const origin = new URL(process.argv[2] || 'http://127.0.0.1:1420');
if (!['127.0.0.1', 'localhost', '[::1]'].includes(origin.hostname)) throw new Error('Startup fixtures require loopback');
const document = `<!doctype html><html><head><script type="module">
import RefreshRuntime from '/@react-refresh';
RefreshRuntime.injectIntoGlobalHook(window);
window.$RefreshReg$ = () => {};
window.$RefreshSig$ = () => type => type;
window.__vite_plugin_react_preamble_installed__ = true;
</script></head><body><div id="fixture"></div><script type="module" src="/__silent_start_fixture.js"></script></body></html>`;
const contextModule = `
import React from '__REACT_MODULE__';
const Context = React.createContext(null);
export function FixtureProvider({ children }) {
  const [app, setApp] = React.useState(() => JSON.parse(sessionStorage.getItem('silent-start-settings') || '{}'));
  const [lockedPaths, setLockedPaths] = React.useState([]);
  const [canSave, setCanSave] = React.useState(true);
  const [snapshotMode, setSnapshotMode] = React.useState('ready');
  window.__lock = setLockedPaths;
  window.__canSave = setCanSave;
  window.__snapshotMode = setSnapshotMode;
  const patchAppSettings = async patch => {
    window.__saveCalls.push(patch);
    if (window.__saveMode === 'pending') await new Promise(resolve => { window.__completeSave = resolve; });
    if (window.__saveMode === 'fail') throw new Error('The test settings store rejected the change');
    const next = { ...app, ...patch.app };
    sessionStorage.setItem('silent-start-settings', JSON.stringify(next));
    setApp(next);
  };
  return React.createElement(Context.Provider, { value: {
    appSettings: snapshotMode === 'missing' ? null : snapshotMode === 'partial' ? {} : { app, policy: { lockedPaths } },
    patchAppSettings,
    personalSettingsStatus: { canSave },
  } }, children);
}
export const useAppState = () => React.useContext(Context);
export const useOptionalAppState = useAppState;
`;
const fixtureModule = `
import React from '__REACT_MODULE__';
import ReactDOM from '/node_modules/.vite/deps/react-dom_client.js';
import '/src/index.css';
import '/src/styles/v2-theme.css';
import '/src/panels/secret/index.css';
import SilentStartSetting from '/src/panels/secret/SilentStartSetting.tsx';
import { FixtureProvider } from '/src/context/AppContext.tsx';
import { MotionPreferenceProvider } from '/src/hooks/useMotionPreference.ts';
import { listNotifications, setPopupAlertsEnabled } from '/src/lib/notificationStore.ts';
setPopupAlertsEnabled(false);
window.__notifications = listNotifications;
window.__saveCalls = [];
window.__saveMode = 'success';
window.__privileges = true;
function Fixture() {
  const [enabled, setEnabled] = React.useState(true);
  const [version, setVersion] = React.useState(0);
  window.__enableAutostart = setEnabled;
  window.__remount = () => setVersion(value => value + 1);
  return React.createElement('div', { className: 'dgz-tile', style: { width: '520px', margin: '24px' } },
    React.createElement(SilentStartSetting, { key: version, autostartEnabled: enabled }));
}
ReactDOM.createRoot(document.getElementById('fixture')).render(
  React.createElement(FixtureProvider, null,
    React.createElement(MotionPreferenceProvider, null, React.createElement(Fixture))));
`;
async function main() {
  const browser = await chromium.launch({ headless: true, executablePath: process.env.WINCOMMANDER_BROWSER_EXECUTABLE || undefined });
  const page = await browser.newPage({ viewport: { width: 1100, height: 700 } });
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  page.setDefaultTimeout(15000);
  const control = page.getByRole('switch', { name: 'Start silently in tray' });
  try {
    const source = await (await page.request.get(new URL('/src/panels/secret/SilentStartSetting.tsx', origin).href)).text();
    const reactModule = source.match(/from "([^"]*\/react\.js[^"]*)"/)?.[1];
    assert.ok(reactModule);
    const privilegeSource = await (await page.request.get(new URL('/src/hooks/useProcessElevation.ts', origin).href)).text();
    const nativeModule = privilegeSource.match(/from "([^"]*tauri-apps_api_core[^"]*)"/)?.[1];
    assert.ok(nativeModule);
    await page.route(new URL(nativeModule, origin).href, route => route.fulfill({ contentType: 'application/javascript', body: `export async function invoke(command) {
      if (command !== 'is_current_process_elevated') throw new Error('Unexpected native command: ' + command);
      if (window.__privileges === 'error') throw new Error('Token unavailable');
      return window.__privileges;
    }` }));
    await page.route('**/src/context/AppContext.tsx*', route => route.fulfill({ contentType: 'application/javascript', body: contextModule.replaceAll('__REACT_MODULE__', reactModule) }));
    await page.route('**/__silent_start_fixture.js', route => route.fulfill({ contentType: 'application/javascript', body: fixtureModule.replaceAll('__REACT_MODULE__', reactModule) }));
    await page.route('**/__silent_start__', route => route.fulfill({ contentType: 'text/html', body: document }));
    await page.goto(new URL('/__silent_start__', origin).href);
    await control.waitFor();
    assert.equal(await control.isChecked(), true, 'Existing settings default to silent');
    await control.click();
    await page.waitForFunction(() => JSON.parse(sessionStorage.getItem('silent-start-settings')).startSilentlyAtSignIn === false);
    assert.equal(await control.isChecked(), false);
    assert.deepEqual(await page.evaluate(() => window.__saveCalls), [{ app: { startSilentlyAtSignIn: false } }]);
    await page.reload();
    await control.waitFor();
    assert.equal(await control.isChecked(), false, 'A reopened settings screen retains saved OFF');
    await page.evaluate(() => { window.__saveMode = 'fail'; });
    await control.click();
    await page.waitForFunction(() => window.__notifications().some(item => item.message.includes('test settings store rejected')));
    assert.equal(await control.isChecked(), false, 'A rejected write cannot leave the switch ON');
    assert.equal(await control.getAttribute('data-state'), 'unchecked');
    assert.equal(await page.evaluate(() => JSON.parse(sessionStorage.getItem('silent-start-settings')).startSilentlyAtSignIn), false);
    await page.evaluate(() => window.__enableAutostart(false));
    await page.waitForFunction(() => document.querySelector('[role="switch"]').disabled);
    assert.equal(await control.isEnabled(), false);
    await page.evaluate(() => { window.__enableAutostart(true); window.__lock(['app.startSilentlyAtSignIn']); });
    await page.getByText('Set by your administrator.', { exact: true }).waitFor();
    assert.equal(await control.isEnabled(), false);
    await page.evaluate(() => { window.__lock([]); window.__canSave(false); });
    assert.equal(await control.isEnabled(), false, 'Unavailable personal settings cannot be overwritten');
    await page.evaluate(() => { window.__canSave(true); window.__saveMode = 'success'; });
    await control.click();
    await page.waitForFunction(() => JSON.parse(sessionStorage.getItem('silent-start-settings')).startSilentlyAtSignIn === true);
    assert.equal(await control.isChecked(), true);
    // A slow native persistence round trip must keep the settings view painted
    // and prevent another request until the authoritative write completes.
    await page.evaluate(() => {
      window.__saveCalls = [];
      window.__saveMode = 'pending';
      window.__settingsView = document.querySelector('.dgz-tile');
    });
    await control.click();
    await page.getByText('Saving…', { exact: true }).waitFor();
    assert.equal(await control.isEnabled(), false);
    assert.equal(await page.evaluate(() => window.__settingsView === document.querySelector('.dgz-tile')), true);
    assert.equal(await control.isChecked(), true, 'Pending OFF keeps the last confirmed ON');
    assert.equal(await page.evaluate(() => window.__saveCalls.length), 1);
    await page.evaluate(() => { window.__saveMode = 'success'; window.__completeSave(); });
    await page.waitForFunction(() => JSON.parse(sessionStorage.getItem('silent-start-settings')).startSilentlyAtSignIn === false);
    for (const expected of [true, false, true, false]) {
      await control.click();
      await page.waitForFunction(value => JSON.parse(sessionStorage.getItem('silent-start-settings')).startSilentlyAtSignIn === value, expected);
      assert.equal(await control.isChecked(), expected);
      assert.equal(await page.evaluate(() => window.__settingsView === document.querySelector('.dgz-tile')), true,
        'Changing the next-start preference must preserve the current settings view');
    }
    assert.deepEqual(await page.evaluate(() => window.__saveCalls), [false, true, false, true, false].map(value => ({ app: { startSilentlyAtSignIn: value } })));
    await page.getByText(/For everyone on this PC/).waitFor();
    const savedCalls = await page.evaluate(() => window.__saveCalls.length);
    await page.evaluate(() => { window.__privileges = false; window.__remount(); });
    await page.getByText('Open WinCommander as administrator to change this PC-wide setting.', { exact: true }).waitFor();
    assert.equal(await control.isEnabled(), false, 'Standard sessions read but cannot change the shared choice');
    assert.equal(await control.isChecked(), false, 'Privilege changes do not replace the machine choice');
    await page.evaluate(() => window.__lock(['app.startSilentlyAtSignIn']));
    await page.getByText('Set by your administrator.', { exact: true }).waitFor();
    assert.equal(await page.getByText('Open WinCommander as administrator to change this PC-wide setting.', { exact: true }).count(), 0);
    await page.evaluate(() => window.__lock([]));
    await page.evaluate(() => { window.__privileges = 'error'; window.__remount(); });
    await page.getByRole('button', { name: 'Check permission again' }).waitFor();
    assert.equal(await control.isEnabled(), false, 'Unverified privileges never enable a machine write');
    await page.evaluate(() => { window.__privileges = true; });
    await page.getByRole('button', { name: 'Check permission again' }).click();
    await page.waitForFunction(() => !document.querySelector('[role="switch"]').disabled);
    assert.equal(await page.evaluate(() => window.__saveCalls.length), savedCalls);
    for (const mode of ['missing', 'partial']) {
      await page.evaluate(value => window.__snapshotMode(value), mode);
      await page.getByText('Waiting for this PC’s startup settings…', { exact: true }).waitFor();
      assert.equal(await control.isEnabled(), false, 'Missing settings cannot be mistaken for a saved machine choice');
    }
    assert.deepEqual(errors, []);
    console.log('PASS: silent default, saved OFF/reopen, failed-write rollback, disabled autostart/policy/unavailable storage, pending persistence, repeated ON/OFF preserve the current view, PC-wide scope, standard/unverified privileges block writes, and permission retry. Browser-local persistence only; no native startup execution.');
  } catch (error) {
    console.error({ errors, fixtureText: await page.locator('body').innerText() });
    throw error;
  } finally { await browser.close(); }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
