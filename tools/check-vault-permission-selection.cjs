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
window.__advanceVaultRevision = () => { policy.version += 1; };
window.__addOtherOwnerVault = () => {
  policy.entries.push({ ...structuredClone(policy.entries[0]), id: 'other-owner-vault', label: 'Other owner vault',
    owner_account: 'OtherAdmin', primary_owner_sid: 'S-1-5-21-111-222-333-1002',
    grants: [{ principal_name: 'OtherAdmin', access: 'write' }],
    container_path: 'D:\\\\Fixture\\\\other.ec', mount: { presentation: 'per-user', preferred_letter: 'M' } });
  policy.version += 1;
};
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
  listAuthorizedEntries: async () => policy.entries.filter(entry => entry.primary_owner_sid === sid).map(entry => ({ entry_id: entry.id, label: entry.label,
    access: 'write', presentation: entry.mount.presentation, container_kind: 'standard',
    mount_state: 'unmounted', drive_letter: null, preferred_letter: entry.mount.preferred_letter })),
  applyOwnerPolicyFragment: async fragment => {
    if (fragment.policy_id !== policy.policy_id || fragment.expected_previous_version !== policy.version) {
      throw new Error('vault policy was changed elsewhere since this draft was loaded');
    }
    const merged = new Map(policy.entries.map(entry => [entry.id, entry]));
    if (!window.__ignoreRemoval) for (const id of fragment.remove_entry_ids || []) merged.delete(id);
    for (const { entry } of fragment.entries) {
      if (entry.primary_owner_sid !== sid) throw new Error('Other-owner records must not be submitted as edits');
      merged.set(entry.id, structuredClone(entry));
    }
    policy = { ...structuredClone(fragment), entries: [...merged.values()] };
    if (!policy.entries.length) policy = { ...policy, policy_id: null, version: 0, expected_previous_version: 0 };
    sessionStorage.setItem('fixture-policy', JSON.stringify(policy));
    window.__lastSavedPattern = policy.entries[0]?.access_pattern;
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
  await page.route('**/src/utils/toast.ts*', route => route.fulfill({
    contentType: 'application/javascript', body: `
      window.__toasts = [];
      export const showSuccess = message => window.__toasts.push({ kind: 'success', message });
      export const showError = message => window.__toasts.push({ kind: 'error', message });
    `
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
      if (pattern === 'shared-write') await page.evaluate(() => window.__advanceVaultRevision());
      await page.getByRole('button', { name: 'Save vault settings', exact: true }).click();
      await page.waitForFunction(expected => window.__lastSavedPattern === expected, pattern);
      await page.waitForFunction(() => !document.querySelector('button[disabled]')?.textContent?.includes('Saving'));
      await page.reload();
      await openEditor();
      assert.equal(await page.locator('[data-vault-access-preset="' + pattern + '"]').getAttribute('aria-checked'), 'true', 'Reload preserves ' + pattern);
    }
    await page.getByRole('button', { name: 'Add private vault', exact: true }).click();
    await page.getByLabel('Vault 2 label', { exact: true }).fill('New vault');
    await page.getByLabel('Vault 2 container path', { exact: true }).fill('D:\\Fixture\\new.ec');
    await page.getByLabel('Vault 2 preferred drive letter', { exact: true }).fill('K');
    await page.evaluate(() => window.__addOtherOwnerVault());
    await page.getByRole('button', { name: 'Save vault settings', exact: true }).click();
    await page.waitForFunction(() => JSON.parse(sessionStorage.getItem('fixture-policy')).entries.length === 3);
    // Restore a starter created before the service already had a saved policy.
    await page.evaluate(() => {
      const saved = JSON.parse(sessionStorage.getItem('fixture-policy'));
      const entry = { ...saved.entries.find(entry => entry.label === 'New vault'), id: 'restored-starter', label: 'Restored starter',
        container_path: 'D:\\Fixture\\restored.ec', mount: { presentation: 'per-user', preferred_letter: 'L' } };
      localStorage.setItem('wincommander.vault-access-draft.v1', JSON.stringify({
        policy: { ...saved, policy_id: 'old-unsaved-starter', version: 0, expected_previous_version: 0, entries: [entry] },
        basePolicy: null,
      }));
    });
    await page.reload();
    await openEditor();
    await page.getByRole('button', { name: 'Save vault settings', exact: true }).click();
    await page.waitForFunction(() => JSON.parse(sessionStorage.getItem('fixture-policy')).entries.length === 4);
    const savedLabels = await page.evaluate(() => JSON.parse(sessionStorage.getItem('fixture-policy')).entries.map(entry => entry.label));
    assert.deepEqual(savedLabels, ['Example vault', 'Other owner vault', 'New vault', 'Restored starter']);
    // An omitted record is not a deletion: click the real saved-row removal flow.
    const savedRow = label => page.locator('tr').filter({ has: page.getByText(label, { exact: true }) });
    await page.evaluate(() => { window.__ignoreRemoval = true; window.__toasts = []; });
    await savedRow('Other owner vault').getByRole('button', { name: 'Remove policy', exact: true }).click();
    await page.getByRole('button', { name: 'Remove and save', exact: true }).click();
    await page.waitForFunction(() => window.__toasts.some(toast => toast.kind === 'error' && toast.message.includes('Removal was not confirmed')));
    assert.equal(await page.evaluate(() => window.__toasts.some(toast => toast.kind === 'success')), false, 'A no-op response must not report removal success');
    await page.evaluate(() => { window.__ignoreRemoval = false; });
    const removeSaved = async label => {
      await savedRow(label).getByRole('button', { name: 'Remove policy', exact: true }).click();
      await page.getByRole('button', { name: 'Remove and save', exact: true }).click();
      await page.waitForFunction(name => !JSON.parse(sessionStorage.getItem('fixture-policy')).entries.some(entry => entry.label === name), label);
      await savedRow(label).waitFor({ state: 'detached' });
    };
    await removeSaved('Other owner vault');
    await page.reload();
    assert.equal(await savedRow('Other owner vault').count(), 0, 'Removed record stays absent after reload');
    await savedRow('New vault').getByRole('button', { name: 'Edit', exact: true }).click();
    await page.getByLabel('Vault 2 label', { exact: true }).fill('Unsent name');
    await page.evaluate(() => window.__addOtherOwnerVault());
    await removeSaved('Example vault');
    assert.equal(await page.evaluate(() => JSON.parse(sessionStorage.getItem('fixture-policy')).entries.find(entry => entry.label === 'New vault')?.label), 'New vault', 'Removal must not submit unrelated draft edits');
    await page.reload();
    await savedRow('Unsent name').waitFor();
    await savedRow('Other owner vault').waitFor();
    await savedRow('Unsent name').getByRole('button', { name: 'Edit', exact: true }).click();
    await page.getByRole('button', { name: 'Save vault settings', exact: true }).click();
    await page.waitForFunction(() => JSON.parse(sessionStorage.getItem('fixture-policy')).entries.some(entry => entry.label === 'Unsent name'));
    assert.equal(await page.evaluate(() => JSON.parse(sessionStorage.getItem('fixture-policy')).entries.some(entry => entry.label === 'Other owner vault')), true, 'Saving retained edits must preserve concurrent additions');
    for (const label of ['Unsent name', 'Restored starter', 'Other owner vault']) await removeSaved(label);
    await page.reload();
    await page.getByText('No vault access is configured yet', { exact: true }).waitFor();
    assert.deepEqual(errors, []);
    console.log('PASS: owner labels; presets and restored drafts; explicit removals persist through reload, including the last vault.');
  } catch (error) {
    console.error({ browserErrors: errors, fixtureText: await page.locator('body').innerText() });
    throw error;
  } finally {
    await browser.close();
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
