// Real recovery components with simulated status; no native IPC or user data.
const assert = require('node:assert/strict');
const { chromium } = require(process.env.WINCOMMANDER_PLAYWRIGHT_MODULE || 'playwright');
const origin = new URL(process.argv[2] || 'http://127.0.0.1:1420');
if (!['127.0.0.1', 'localhost', '[::1]'].includes(origin.hostname)) {
  throw new Error('Recovery UI fixtures require a loopback server');
}
const fixture = `<!doctype html><html><head>
<script type="module">
import RefreshRuntime from '/@react-refresh';
RefreshRuntime.injectIntoGlobalHook(window);
window.$RefreshReg$ = () => {};
window.$RefreshSig$ = () => type => type;
window.__vite_plugin_react_preamble_installed__ = true;
</script></head><body><div id="fixture"></div>
<script type="module" src="/__recovery_notice_fixture.js"></script></body></html>`;
const fixtureModule = `
import React from '/node_modules/.vite/deps/react.js';
import ReactDOM from '/node_modules/.vite/deps/react-dom_client.js';
import '/src/index.css';
import '/src/styles/v2-theme.css';
import { PersonalSettingsNotice, PersonalSettingsRecoveryDetails } from '/src/components/startup/PersonalSettingsNotice.tsx';
function Fixture() {
  const [status, setStatus] = React.useState({ mode: 'service', recoveryRequired: true, canSave: true });
  const [showSettings, setShowSettings] = React.useState(false);
  window.updateRecoveryFixture = next => setStatus(next);
  window.readRecoveryFixture = () => status;
  return React.createElement(React.Fragment, null,
    React.createElement(PersonalSettingsNotice, { status, onOpenSettings: () => setShowSettings(true) }),
    React.createElement('button', { onClick: () => setShowSettings(true) }, 'Open Settings'),
    showSettings && React.createElement(PersonalSettingsRecoveryDetails, { status }));
}
ReactDOM.createRoot(document.getElementById('fixture')).render(React.createElement(Fixture));
`;

async function main() {
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  page.setDefaultTimeout(30000);
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('requestfailed', request => errors.push(`${new URL(request.url()).pathname}: ${request.failure()?.errorText}`));
  // Vite's optimized React module owns its instance; its version query must match.
  const source = await (await page.request.get(new URL('/src/components/startup/PersonalSettingsNotice.tsx', origin).href)).text();
  const reactModule = source.match(/from "([^"]*\/react\.js[^"]*)"/)?.[1];
  assert.ok(reactModule, 'Resolve the same React module used by the real component');
  const moduleBody = fixtureModule.replace('/node_modules/.vite/deps/react.js', reactModule);
  await page.route('**/__recovery_notice_fixture.js', route => route.fulfill({ contentType: 'application/javascript', body: moduleBody }));
  await page.route('**/__recovery_notice__', route => route.fulfill({ contentType: 'text/html', body: fixture }));
  const dismiss = () => page.getByRole('button', { name: 'Dismiss personal data recovery notice for this session', exact: true });
  const details = () => page.getByRole('region', { name: 'Personal data recovery', exact: true });
  const initial = { mode: 'service', recoveryRequired: true, canSave: true };
  try {
    await page.goto(new URL('/__recovery_notice__', origin).href, { waitUntil: 'domcontentloaded', timeout: 60000 });
    await dismiss().waitFor();
    await page.getByRole('button', { name: 'Review in Settings', exact: true }).click();
    await details().waitFor();
    await dismiss().click();
    await page.getByRole('status').waitFor({ state: 'detached' });
    assert.equal(await details().count(), 1, 'Settings recovery details remain after banner dismissal');
    assert.deepEqual(await page.evaluate(() => window.readRecoveryFixture()), initial, 'Dismissal cannot change recovery or save status');
    await page.evaluate(status => window.updateRecoveryFixture({ ...status }), initial);
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    assert.equal(await dismiss().count(), 0, 'An ordinary refresh does not restore an acknowledged banner');

    await page.evaluate(status => window.updateRecoveryFixture({ ...status, canSave: false }), initial);
    await dismiss().waitFor();
    assert.ok((await page.getByRole('status').innerText()).includes('cannot be saved right now'));
    await dismiss().focus();
    await page.keyboard.press('Enter');
    await page.getByRole('status').waitFor({ state: 'detached' });
    assert.equal(await details().count(), 1, 'Keyboard dismissal keeps Settings accessible');

    await page.evaluate(status => window.updateRecoveryFixture({ ...status, recoveryRequired: false }), initial);
    await details().waitFor({ state: 'detached' });
    assert.equal(await page.getByRole('status').count(), 0, 'Healthy status hides recovery UI');
    await page.evaluate(status => window.updateRecoveryFixture(status), initial);
    await dismiss().waitFor();
    await dismiss().click();
    await page.getByRole('status').waitFor({ state: 'detached' });
    await page.reload();
    await dismiss().waitFor();
    assert.deepEqual(errors, [], 'Recovery components must not raise browser errors');
    console.log('PASS: actual pointer/keyboard dismissal, Settings details, unchanged protection status, ordinary refresh, changed condition, recovery recurrence, and new-session redisplay. Browser fixture only; no native recovery performed.');
  } catch (error) {
    console.error('Fixture browser errors:', errors);
    throw error;
  } finally {
    await browser.close();
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
