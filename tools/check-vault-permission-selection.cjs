// Isolated browser regression: real Fleet components, simulated service only.
const assert = require('node:assert/strict');
const { chromium } = require(process.env.WINCOMMANDER_PLAYWRIGHT_MODULE || 'playwright');
const origin = new URL(process.argv[2] || 'http://127.0.0.1:1420');
if (!['127.0.0.1', 'localhost', '[::1]'].includes(origin.hostname)) {
  throw new Error('Vault UI fixtures require a loopback server');
}
const fixture = `<!doctype html><html><head>
<script type="module">
import RefreshRuntime from '/@react-refresh';
RefreshRuntime.injectIntoGlobalHook(window);
window.$RefreshReg$ = () => {};
window.$RefreshSig$ = () => type => type;
window.__vite_plugin_react_preamble_installed__ = true;
</script></head><body><div id="fixture"></div>
<script type="module" src="/__vault_selection_fixture.js"></script></body></html>`;
const fixtureModule = `
import React from '/node_modules/.vite/deps/react.js';
import ReactDOM from '/node_modules/.vite/deps/react-dom_client.js';
import '/src/index.css';
import '/src/styles/v2-theme.css';
import '/src/panels/fleet/index.css';
import VaultAccessTab from '/src/panels/fleet/VaultAccessTab.tsx';
const sid = 'S-1-5-21-111-222-333-1001';
let policy = JSON.parse(sessionStorage.getItem('fixture-policy') || 'null') || {
  schema_version: 1, policy_id: 'fixture', version: 1, expected_previous_version: 0,
  entries: [{ id: 'fixture-vault', label: 'Example vault', container_path: 'D:\\\\Fixture\\\\example.ec',
    container_kind: 'standard', owner_account: 'ExampleUser', primary_owner_sid: sid,
    access_pattern: 'private', grants: [{ principal_name: 'ExampleUser', access: 'write' }],
    mount: { presentation: 'per-user', preferred_letter: 'J' } }]
};
const status = () => ({ policy_id: policy.policy_id, version: policy.version,
  validation_state: 'current', applied_at: 1,
  entries: policy.entries.map(entry => ({ id: entry.id, result: 'applied', mount_state: 'unmounted' })) });
const blocked = async () => { throw new Error('Native mutation blocked by fixture'); };
window.__vaultUiService = {
  getCapabilities: async () => ({ can_manage_policy: true }),
  listOwnerPrincipals: async () => ({ current_caller_sid: sid, principals: [
    { sid, display_name: 'ExampleUser', is_local_administrator: true },
    { sid: 'S-1-5-21-111-222-333-1002', display_name: 'OtherAdmin', is_local_administrator: true }
  ] }),
  getOwnerPolicyFragment: async () => ({ ...structuredClone(policy), entries: policy.entries.map(entry => ({
    entry: structuredClone(entry), container_path_state: 'available', canonical_container_path: entry.container_path
  })) }),
  getStatus: async () => status(),
  listAuthorizedEntries: async () => policy.entries.map(entry => ({ entry_id: entry.id, label: entry.label,
    access: 'write', presentation: entry.mount.presentation, container_kind: 'standard',
    mount_state: 'unmounted', drive_letter: null, preferred_letter: entry.mount.preferred_letter })),
  applyOwnerPolicyFragment: async fragment => {
    policy = { ...structuredClone(fragment), entries: fragment.entries.map(owned => owned.entry) };
    sessionStorage.setItem('fixture-policy', JSON.stringify(policy));
    window.__lastSavedPattern = policy.entries[0].access_pattern;
    return status();
  },
  mountEntry: blocked, unmountEntry: blocked, forgetPolicy: blocked
};
ReactDOM.createRoot(document.getElementById('fixture')).render(React.createElement(VaultAccessTab, {
  isAdmin: true, directory: { schema: 1, users: [
    { id: 'example', username: 'ExampleUser', displayName: 'ExampleUser' },
    { id: 'other', username: 'OtherAdmin', displayName: 'OtherAdmin' }
  ], groups: [] }
}));
`;

async function main() {
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  page.setDefaultTimeout(15000);
  await page.route('**/src/hooks/useVaultAccess.ts*', route => route.fulfill({
    contentType: 'application/javascript', body: 'export default function useVaultAccess(){return window.__vaultUiService;}'
  }));
  await page.route('**/__vault_selection_fixture.js', route => route.fulfill({ contentType: 'application/javascript', body: fixtureModule }));
  await page.route('**/__vault_selection__', route => route.fulfill({ contentType: 'text/html', body: fixture }));
  const openEditor = async () => {
    await page.getByRole('button', { name: 'Edit', exact: true }).click();
    await page.locator('.vault-access-editor').waitFor();
  };
  try {
    await page.goto(new URL('/__vault_selection__', origin).href);
    await openEditor();
    const owner = page.locator('.vault-access-editor select').first();
    const labels = await owner.locator('option').allTextContents();
    assert.ok(labels.some(label => label.includes('ExampleUser') && /current user/i.test(label)));
    assert.ok(labels.every(label => !label.includes('S-1-5-')), 'Owner labels must not show SID numbers');
    const writeChoice = page.locator('[data-vault-access-preset="shared-write"]');
    await writeChoice.click();
    assert.equal(await writeChoice.getAttribute('aria-checked'), 'true', 'Single-owner starter must retain the third choice');
    // A shared policy needs a second named grant before it is valid to save.
    await page.getByRole('button', { name: 'Add person or group', exact: true }).click();
    await page.getByLabel('Grant 2 principal', { exact: true }).selectOption('OtherAdmin');
    for (const pattern of ['shared-write', 'shared-read', 'private']) {
      const choice = page.locator('[data-vault-access-preset="' + pattern + '"]');
      await choice.click();
      assert.equal(await choice.getAttribute('aria-checked'), 'true', 'Click selects ' + pattern);
      await page.getByRole('button', { name: 'Save vault settings', exact: true }).click();
      await page.waitForFunction(expected => window.__lastSavedPattern === expected, pattern);
      await page.waitForFunction(() => !document.querySelector('button[disabled]')?.textContent?.includes('Saving'));
      await page.reload();
      await openEditor();
      assert.equal(await page.locator('[data-vault-access-preset="' + pattern + '"]').getAttribute('aria-checked'), 'true', 'Reload preserves ' + pattern);
    }
    assert.deepEqual(errors, []);
    console.log('PASS: owner dropdown labels; real clicks on all three modes; save and page reload preserve exact selection.');
  } catch (error) {
    console.error({ browserErrors: errors, fixtureText: await page.locator('body').innerText() });
    throw error;
  } finally {
    await browser.close();
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
