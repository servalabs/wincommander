// Browser integration only; fake machine store, no Windows/service mutations.
const assert = require('node:assert/strict');
const { chromium } = require(process.env.WINCOMMANDER_PLAYWRIGHT_MODULE || 'playwright');
const origin = new URL(process.argv[2] || 'http://127.0.0.1:5173');
if (!['127.0.0.1', 'localhost', '[::1]'].includes(origin.hostname)) throw Error('Loopback server required');
const html = `<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1"><script type="module">
import RefreshRuntime from '/@react-refresh';RefreshRuntime.injectIntoGlobalHook(window);
window.$RefreshReg$=()=>{};window.$RefreshSig$=()=>type=>type;window.__vite_plugin_react_preamble_installed__=true;
</script></head><body><div id="fixture"></div></body></html>`;

async function main() {
  let durable = { schema_version: 1, users: [{ sid: 'S-1-5-21-101', username: 'Alice' }], groups: [{ id: 'team', name: 'Initial Team', local_group: 'WC_Team', member_sids: ['S-1-5-21-101'] }] };
  let writes = 0;
  const browser = await chromium.launch({ headless: true, ...(process.env.WINCOMMANDER_PLAYWRIGHT_EXECUTABLE_PATH ? { executablePath: process.env.WINCOMMANDER_PLAYWRIGHT_EXECUTABLE_PATH } : {}) });
  const open = async () => {
    const context = await browser.newContext({ viewport: { width: 1280, height: 850 } });
    await context.exposeBinding('__readMachineGroups', () => structuredClone(durable));
    await context.exposeBinding('__saveMachineGroups', (_source, value) => {
      writes++; durable = structuredClone(value);
      return { directory: structuredClone(durable), results: value.groups.map(group => ({ local_group: group.local_group, state: 'updated', error: null })) };
    });
    const js = body => ({ contentType: 'application/javascript', body });
    await context.route('**/src/hooks/useBackend.ts*', route => route.fulfill(js('export default function(){return window.__groupBackend}')));
    await context.route('**/src/hooks/useVaultAccess.ts*', route => route.fulfill(js('export default function(){return window.__groupService}')));
    await context.route('**/src/utils/toast.ts*', route => route.fulfill(js('export const showSuccess=async(message)=>window.__groupNotices.push({kind:"success",message});export const showError=async(message)=>window.__groupNotices.push({kind:"error",message});export const showWarning=async(message)=>window.__groupNotices.push({kind:"warning",message});')));
    await context.route('**/src/panels/fleet/FleetConnectView.tsx*', route => route.fulfill(js('export default function(){return null}')));
    await context.route('**/src/panels/fleet/VaultAccessTab.tsx*', route => route.fulfill(js('import React from "/node_modules/.vite/deps/react.js";export default function({directory}){return React.createElement("div",{"data-testid":"reusable-groups"},directory.groups.map(g=>g.name).join(", "))}')));
    await context.route('**/__fleet_groups_ui__', route => route.fulfill({ contentType: 'text/html', body: html }));
    const page = await context.newPage();
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    await page.goto(new URL('/__fleet_groups_ui__', origin).href);
    await page.waitForFunction(() => window.__vite_plugin_react_preamble_installed__);
    await page.evaluate(() => import('/tools/fixtures/fleet-groups-ui.js'));
    await page.getByRole('tab', { name: 'Access control', exact: true }).click();
    await page.getByLabel('Group name', { exact: true }).waitFor();
    return { page, context, errors };
  };
  try {
    const a = await open();
    await a.page.getByLabel('Group name', { exact: true }).fill('Saved Team');
    await a.page.evaluate(() => { window.__groupStallDiscovery = true; });
    await a.page.getByRole('button', { name: 'Save groups', exact: true }).click();
    await a.page.getByRole('status').filter({ hasText: 'Group "Saved Team" saved on this PC' }).waitFor();
    assert.equal(await a.page.evaluate(() => window.__groupNotices.some(notice => notice.kind === 'success')), true);
    assert.equal(durable.groups[0].name, 'Saved Team');
    assert.equal(writes, 1);
    await a.page.evaluate(() => { window.__groupStallDiscovery = false; window.__releaseGroupDiscovery?.(); });
    const b = await open();
    assert.equal(await b.page.getByLabel('Group name', { exact: true }).inputValue(), 'Saved Team');
    await b.page.getByRole('tab', { name: 'Vault permissions', exact: true }).click();
    assert.match(await b.page.getByTestId('reusable-groups').innerText(), /Saved Team/);
    await b.page.getByRole('tab', { name: 'Access control', exact: true }).click();
    durable.groups[0].name = 'Another admin change';
    await b.page.evaluate(() => window.dispatchEvent(new Event('focus')));
    await b.page.waitForFunction(() => document.querySelector('.fleet-access-fields input')?.value === 'Another admin change');
    await b.page.getByLabel('Group name', { exact: true }).fill('Unsaved local edit');
    durable.groups[0].name = 'Later machine change';
    await b.page.evaluate(() => window.dispatchEvent(new Event('focus')));
    assert.equal(await b.page.getByLabel('Group name', { exact: true }).inputValue(), 'Unsaved local edit');
    await b.page.getByRole('button', { name: 'Refresh groups', exact: true }).click();
    await b.page.getByRole('button', { name: 'Discard draft and refresh', exact: true }).click();
    await b.page.waitForFunction(() => document.querySelector('.fleet-access-fields input')?.value === 'Later machine change');
    for (const width of [720, 360]) {
      await b.page.setViewportSize({ width, height: 800 });
      assert.equal(await b.page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 2), true);
    }
    assert.deepEqual([...a.errors, ...b.errors], []);
    console.log('PASS: real Fleet parent/group UI save notification precedes stalled discovery; fresh contexts, focus, Vault reuse and explicit refresh share one fake machine store; dirty drafts preserved; responsive widths pass. No native group changes.');
  } finally { await browser.close(); }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
