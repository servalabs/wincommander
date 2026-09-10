// Run against a loopback Vite server. All service results below are synthetic;
// no native IPC, credentials, actual Vaults, screenshots, or private paths are used.
const assert = require('node:assert/strict');
const { chromium } = require(process.env.WINCOMMANDER_PLAYWRIGHT_MODULE || 'playwright');
const origin = new URL(process.argv[2] || 'http://127.0.0.1:5173');
if (!['127.0.0.1', 'localhost', '[::1]'].includes(origin.hostname)) {
  throw new Error('Vault UI fixtures require a loopback server');
}

const fixture = `<!doctype html><html class="dark"><head><meta name="viewport" content="width=device-width,initial-scale=1"></head><body>
<div id="fixture"></div><script type="module">
import RefreshRuntime from '/@react-refresh';
RefreshRuntime.injectIntoGlobalHook(window);
window.$RefreshReg$ = () => {};
window.$RefreshSig$ = () => type => type;
window.__vite_plugin_react_preamble_installed__ = true;
await import('/src/index.css');
await import('/src/styles/v2-theme.css');
await import('/src/panels/fleet/index.css');
const React = await import('/node_modules/.vite/deps/react.js');
const { createRoot } = await import('/node_modules/.vite/deps/react-dom_client.js');
const { default: VaultAccessTab } = await import('/src/panels/fleet/VaultAccessTab.tsx');
const { writeVaultAccessDraft } = await import('/src/panels/fleet/vaultAccessDraft.ts');
const style = document.createElement('style');
style.textContent = 'html,body,#fixture{height:100%;margin:0} #fixture{overflow-y:auto} .fixture-panel{min-height:100%;height:auto;padding:16px;box-sizing:border-box}';
document.head.append(style);
let root;
window.renderVaultFixture = state => {
  root?.unmount();
  localStorage.clear();
  const entry = {id:'example-vault',label:'Example vault',container_path:'',container_kind:state === 'dual' ? 'dual' : 'standard',owner_account:'ExampleUser',grants:[{principal_name:'ExampleTeam',access:'read'},{principal_name:'ExampleUser',access:'write'}],mount:{presentation:'machine'}};
  const policy = {schema:1,policy_id:'example-policy',version:1,entries:[entry]};
  const status = {policy_id:policy.policy_id,version:1,validation_state:state === 'degraded' ? 'degraded' : 'current',applied_at:1,entries:[{id:entry.id,result:state === 'degraded' ? 'acl_readback_failed' : 'applied'}]};
  const authorized = {entry_id:entry.id,label:entry.label,access:'write',presentation:'machine',container_kind:entry.container_kind,mount_state:state === 'mounted' ? 'mounted' : 'unmounted',drive_letter:null};
  const blocked = async () => { throw new Error('Native mutation blocked by UI fixture'); };
  window.__vaultUiService = {
    getPolicy:async () => { if(state === 'unavailable') throw new Error('Synthetic unavailable state'); return structuredClone(policy); },
    getStatus:async () => structuredClone(status),
    getCapabilities:async () => ({can_manage_policy:state !== 'unelevated'}),
    listAuthorizedEntries:async () => state === 'unauthorized' ? [] : [structuredClone(authorized)],
    applyPolicy:blocked,mountEntry:blocked,unmountEntry:blocked
  };
  if(state === 'draft') writeVaultAccessDraft({...policy,entries:[{...entry,label:'Example draft'}]},undefined,policy);
  const directory = {schema:1,users:[{id:'example-user',username:'ExampleUser',displayName:'Example user'}],groups:[{id:'example-group',name:'Example team',localGroup:'ExampleTeam',userIds:[]}]};
  root = createRoot(document.getElementById('fixture'));
  root.render(React.createElement('div',{className:'panel-container fleet-panel fixture-panel'},React.createElement(VaultAccessTab,{isAdmin:true,directory})));
};
window.renderVaultFixture('saved');
</script></body></html>`;

async function main() {
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  page.setDefaultTimeout(15000);
  const browserErrors = [];
  page.on('pageerror', error => browserErrors.push(error.name));
  await page.route('**/src/hooks/useVaultAccess.ts*', route => route.fulfill({
    contentType: 'application/javascript', body: 'export default function useVaultAccess(){return window.__vaultUiService;}'
  }));
  await page.route('**/__vault_access_ui__', route => route.fulfill({ contentType: 'text/html', body: fixture }));
  const reset = async state => {
    await page.evaluate(value => window.renderVaultFixture(value), state);
    await page.getByText('Loading your Vault access…').waitFor({ state: 'hidden' });
    if (!['unelevated', 'unavailable'].includes(state)) await page.locator('.vault-access-editor').waitFor();
  };
  const closed = () => page.locator('.vault-access-details').evaluate(element => !element.open);
  try {
    await page.goto(new URL('/__vault_access_ui__', origin).href);
    await page.locator('.vault-access-editor').waitFor();
    const details = page.locator('.vault-access-details');
    const summary = details.locator('summary');
    assert.equal(await closed(), true, 'Details must start closed');
    await summary.click();
    assert.equal(await closed(), false, 'Pointer opens details');
    await summary.click();
    assert.equal(await closed(), true, 'Pointer closes details');
    await summary.focus();
    await page.keyboard.press('Enter');
    assert.equal(await closed(), false, 'Keyboard opens details');
    await page.keyboard.press('Space');
    assert.equal(await closed(), true, 'Keyboard closes details');

    const info = page.getByRole('button', { name: 'About primary owner', exact: true });
    const editor = page.locator('.vault-access-editor');
    const initialHeight = (await editor.boundingBox()).height;
    await info.hover();
    await page.getByRole('tooltip').waitFor();
    assert.ok(Math.abs((await editor.boundingBox()).height - initialHeight) < 1, 'Hover help must not shift the editor');
    await page.keyboard.press('Escape');
    await page.getByRole('tooltip').waitFor({ state: 'hidden' });
    await page.mouse.move(0, 0);
    await summary.focus();
    await info.focus();
    await page.getByRole('tooltip').waitFor();
    assert.ok(await info.getAttribute('aria-describedby'), 'Focused Info must describe its control');
    await page.keyboard.press('Escape');
    await page.getByRole('tooltip').waitFor({ state: 'hidden' });
    assert.equal(await info.evaluate(element => element === document.activeElement), true, 'Escape retains focus');

    await summary.click();
    await page.getByRole('button', { name: 'Manage access', exact: true }).click();
    await page.waitForFunction(() => document.activeElement?.getAttribute('aria-label') === 'Grant 1 principal');
    assert.equal(await closed(), true, 'Manage access resets details');
    await summary.click();
    await page.getByRole('button', { name: 'Edit', exact: true }).click();
    await page.waitForFunction(() => document.activeElement?.getAttribute('aria-label') === 'Vault 1 label');
    assert.equal(await closed(), true, 'Edit resets details');
    const row = page.getByRole('group', { name: 'Permission 1', exact: true });
    assert.ok((await row.ariaSnapshot()).includes('Grant 1 principal'), 'Principal is named in the accessibility tree');
    await page.getByLabel('Grant 1 principal', { exact: true }).focus();
    await page.keyboard.press('Tab');
    assert.equal(await page.getByLabel('Grant 1 access', { exact: true }).evaluate(element => element === document.activeElement), true, 'Tab moves from principal to access');
    await page.keyboard.press('ArrowDown');
    assert.equal(await page.getByLabel('Grant 1 access', { exact: true }).inputValue(), 'write', 'Keyboard can edit access');
    assert.equal(await page.getByText('Draft auto-saved on this PC — not yet applied to Windows.').isVisible(), true);

    for (const width of [1440, 720, 360]) {
      await reset('saved');
      await page.setViewportSize({ width, height: 900 });
      const geometry = await editor.evaluate(element => {
        const boundary = element.getBoundingClientRect();
        const fields = [...element.querySelectorAll('input,select,.fleet-vault-grant-row,summary')];
        return {
          fits: fields.every(field => { const box = field.getBoundingClientRect(); return box.left >= boundary.left - 1 && box.right <= boundary.right + 1 && box.width > 20; }),
          overlaps: [...element.querySelectorAll('.fleet-vault-grant-row')].some(rowElement => {
            const boxes = [...rowElement.children].map(child => child.getBoundingClientRect());
            return boxes.some((a, i) => boxes.slice(i + 1).some(b => a.left < b.right - 1 && a.right > b.left + 1 && a.top < b.bottom - 1 && a.bottom > b.top + 1));
          }),
          scrollOwners: [...element.querySelectorAll('*')].filter(child => /auto|scroll/.test(getComputedStyle(child).overflowY) && child.scrollHeight > child.clientHeight + 1).length
        };
      });
      assert.equal(geometry.fits, true, `Controls fit at ${width}px`);
      assert.equal(geometry.overlaps, false, `Rows do not overlap at ${width}px`);
      assert.equal(geometry.scrollOwners, 0, `Editor has no nested scrolling at ${width}px`);
      await info.focus();
      await page.getByRole('tooltip').waitFor();
      const popup = await page.locator('.vault-access-info-content').boundingBox();
      assert.ok(popup.x >= 0 && popup.x + popup.width <= width + 1, `Info fits at ${width}px`);
      await page.keyboard.press('Escape');
    }

    await page.setViewportSize({ width: 1440, height: 900 });
    for (const [state, text] of [
      ['saved', 'Showing the policy saved by the security service.'],
      ['draft', 'Draft auto-saved on this PC — not yet applied to Windows.'],
      ['unauthorized', 'This Windows account is not authorized to mount this vault.'],
      ['dual', 'A writable outer mount requires the hidden protection password for that one request.']
    ]) {
      await reset(state);
      assert.equal(await closed(), true);
      assert.equal(await page.getByText(text, { exact: true }).isVisible(), true, `${state} message remains visible`);
    }
    await reset('degraded');
    assert.equal(await closed(), true);
    assert.equal(await page.getByRole('alert').filter({ hasText: 'Mounting is unavailable until this is fixed' }).isVisible(), true, 'Degraded warning stays visible');
    assert.equal(await page.locator('.fleet-vault-workspace').getByRole('button', { name: 'Mount', exact: true }).isDisabled(), true);
    await reset('unelevated');
    assert.equal(await page.getByRole('alert').filter({ hasText: 'Run WinCommander as administrator' }).isVisible(), true);
    await reset('unavailable');
    assert.equal(await page.getByRole('alert').filter({ hasText: 'Vault settings could not be loaded yet' }).isVisible(), true);
    await reset('mounted');
    assert.equal(await page.locator('.fleet-vault-workspace').getByText('Mounted for this Windows session', { exact: true }).isVisible(), true);
    await reset('saved');
    const mount = page.locator('.fleet-vault-workspace').getByRole('button', { name: 'Mount', exact: true });
    await mount.click();
    await page.getByRole('dialog').waitFor();
    await page.keyboard.press('Escape');
    await page.getByRole('dialog').waitFor({ state: 'hidden' });
    assert.deepEqual(browserErrors, [], 'Fixture must not produce browser exceptions');
    console.log('Vault UI PASS: disclosure, hover/focus/Escape, accessibility, keyboard, 3 widths, warnings, saved/draft/unauthorized/degraded/mounted states, modal Escape.');
  } finally {
    await browser.close();
  }
}
main().catch(error => { console.error(error.message); process.exitCode = 1; });
