// Real dashboard finding components/hooks with simulated Windows replies only.
const assert = require('node:assert/strict');
const { chromium } = require(process.env.WINCOMMANDER_PLAYWRIGHT_MODULE || 'playwright');
const origin = new URL(process.argv[2] || 'http://127.0.0.1:1420');
if (!['127.0.0.1', 'localhost', '[::1]'].includes(origin.hostname)) throw new Error('Dashboard fixtures require loopback');
const fixture = `<!doctype html><html><head><script type="module">
import RefreshRuntime from '/@react-refresh';
RefreshRuntime.injectIntoGlobalHook(window);
window.$RefreshReg$ = () => {};
window.$RefreshSig$ = () => type => type;
window.__vite_plugin_react_preamble_installed__ = true;
</script></head><body><div id="fixture"></div><script type="module" src="/__dashboard_fix_fixture.js"></script></body></html>`;
const fixtureModule = `
import React from '/node_modules/.vite/deps/react.js';
import ReactDOM from '/node_modules/.vite/deps/react-dom_client.js';
import '/src/index.css';
import '/src/styles/v2-theme.css';
import NeedsAttention from '/src/components/dashboard/NeedsAttention.tsx';
import { useFindingFixAttempts } from '/src/panels/dashboard/useFindingFixAttempts.ts';
import { retainUnverifiedFindings, verifyDashboardToggleFix } from '/src/panels/dashboard/fixVerification.ts';
import { DASHBOARD_POLICY_FIELDS } from '/src/lib/dashboardPolicyObservation.ts';
import { getToggleById } from '/src/registry/index.ts';
const ids = Object.keys(DASHBOARD_POLICY_FIELDS);
const findings = ids.map(id => ({ id, label: getToggleById(id).label, impact: getToggleById(id).impact, category: 'privacy', severity: 'warning' }));
window.__fixModes = { recallSnapshots: 'ack', internetComm: 'mismatch', officeLog: 'unknown', bitlockerAuto: 'verified' };
window.__verifiedFixes = JSON.parse(sessionStorage.getItem('fixture-verified') || '[]');
function Fixture() {
  const [cached, setCached] = React.useState(() => Object.fromEntries(ids.map(id => [id, window.__verifiedFixes.includes(id)])));
  const [busy, setBusy] = React.useState(new Set());
  const [ignored, setIgnored] = React.useState([]);
  const { fixAttempts, trackFindingFix } = useFindingFixAttempts();
  const fix = async finding => {
    setBusy(new Set([finding.id]));
    try {
      await trackFindingFix(finding, async () => {
        // Reproduce an old optimistic cache update racing with independent readback.
        setCached(current => ({ ...current, [finding.id]: true }));
        const mode = window.__fixModes[finding.id];
        const result = { success: true, data: mode === 'ack' ? { status: 'disabled' } : { status: 'disabled', verified: true } };
        await verifyDashboardToggleFix(finding.id, true, result, async () => ({ success: true, data: {
          [DASHBOARD_POLICY_FIELDS[finding.id]]: mode === 'verified' ? true : mode === 'unknown' ? null : false
        } }));
        window.__verifiedFixes.push(finding.id);
        sessionStorage.setItem('fixture-verified', JSON.stringify(window.__verifiedFixes));
      });
    } catch { /* inline failure is owned by the real hook; no notifications */ }
    finally { setBusy(new Set()); }
  };
  const visible = retainUnverifiedFindings(findings.filter(finding => !cached[finding.id]), fixAttempts).filter(finding => !ignored.includes(finding.id));
  return React.createElement(NeedsAttention, {
    findings: visible, busyIds: busy,
    fixErrors: Object.fromEntries(Object.entries(fixAttempts).filter(([, value]) => value.error).map(([id, value]) => [id, value.error])),
    onFixOne: fix, onFixAll: () => {},
    ignoredFindingIds: ignored, knownFindings: findings,
    onIgnore: finding => setIgnored(current => [...current, finding.id]),
    onRestoreIgnored: id => setIgnored(current => current.filter(value => value !== id)),
  });
}
ReactDOM.createRoot(document.getElementById('fixture')).render(React.createElement(Fixture));
`;
async function main() {
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  page.setDefaultTimeout(15000);
  const row = label => page.locator('.na-item').filter({ hasText: label });
  try {
    const source = await (await page.request.get(new URL('/src/components/dashboard/NeedsAttention.tsx', origin).href)).text();
    const reactModule = source.match(/from "([^"]*\/react\.js[^"]*)"/)?.[1];
    assert.ok(reactModule);
    await page.route('**/__dashboard_fix_fixture.js', route => route.fulfill({ contentType: 'application/javascript', body: fixtureModule.replace('/node_modules/.vite/deps/react.js', reactModule) }));
    await page.route('**/__dashboard_fix__', route => route.fulfill({ contentType: 'text/html', body: fixture }));
    await page.goto(new URL('/__dashboard_fix__', origin).href);
    await page.locator('.na-item').first().waitFor();
    const labels = await page.locator('.na-label').allTextContents();
    assert.equal(labels.length, 4);
    const scenarios = [
      ['Recall', 'Windows has not confirmed'],
      ['Internet', 'Windows still reports'],
      ['Office', 'could not be checked afterwards'],
    ];
    for (const [label, message] of scenarios) {
      await row(label).getByRole('button', { name: 'Fix', exact: true }).click();
      await row(label).getByRole('alert').filter({ hasText: message }).waitFor();
      assert.equal(await page.locator('.na-item').count(), 4, 'Unverified rows survive optimistic snapshots');
    }
    await row('BitLocker').getByRole('button', { name: 'Fix', exact: true }).click();
    await row('BitLocker').waitFor({ state: 'detached' });
    assert.deepEqual(await page.evaluate(() => window.__verifiedFixes), ['bitlockerAuto']);
    await row('Recall').getByRole('button', { name: 'Ignore', exact: true }).click();
    await page.getByRole('button', { name: 'Ignored (1)', exact: true }).waitFor();
    assert.deepEqual(await page.evaluate(() => window.__verifiedFixes), ['bitlockerAuto'], 'Ignore never reports a verified fix');
    await page.reload();
    await row('Recall').waitFor();
    assert.equal(await page.locator('.na-item').count(), 3, 'Reopening retains only truly persisted success');
    assert.equal(await row('BitLocker').count(), 0);
    await page.evaluate(() => { window.__fixModes.officeLog = 'verified'; });
    await row('Office').getByRole('button', { name: 'Fix', exact: true }).click();
    await row('Office').waitFor({ state: 'detached' });
    assert.deepEqual(await page.evaluate(() => window.__verifiedFixes), ['bitlockerAuto', 'officeLog']);
    assert.deepEqual(errors, []);
    console.log('PASS: acknowledgement, mismatch and unknown readback stay visible with inline errors; exact verified readback clears rows; Ignore never counts as fixed; reload preserves only verified state. Simulated native responses only.');
  } catch (error) {
    console.error({ errors, fixtureText: await page.locator('body').innerText() });
    throw error;
  } finally { await browser.close(); }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
