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
  entries: policy.entries.map(entry => ({ id: entry.id, result: 'applied', mount_state: window.__fixtureMounted ? 'mounted' : 'unmounted' })) });
const blocked = async () => { throw new Error('Native mutation blocked by fixture'); };
window.__vaultBackend = {
  getAvailableDriveLetters: async excludeEntryId => ({ success: true, data: { letters: ['J', 'K', 'L', 'M', 'V', 'W', 'Z'].filter(letter =>
    !(window.__occupiedLetters || []).includes(letter) && !policy.entries.some(entry => entry.id !== excludeEntryId && entry.mount.preferred_letter === letter)) } }),
  openEncryptionVolume: async drive => { window.__openedDrive = drive; return { success: true }; }
};
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
    mount_state: window.__fixtureMounted ? 'mounted' : 'unmounted', drive_letter: window.__fixtureMounted ? 'J' : null, preferred_letter: entry.mount.preferred_letter })),
  applyOwnerPolicyFragment: async fragment => {
    if (window.__policyError) throw new Error(window.__policyError);
    if (window.__ignoreSave) return status();
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
  mountEntry: async entry_id => {
    if (window.__mountReason) return { entry_id, state: 'failed', presentation: null, drive_letter: null, reason: window.__mountReason };
    if (window.__reportAlreadyMounted) return { entry_id, state: 'failed', presentation: null, drive_letter: null, reason: 'already_mounted' };
    if (window.__allowFixtureMount) {
      if (!window.__mountAckOnly) window.__fixtureMounted = true;
      return { entry_id, state: 'mounted', presentation: 'per-user', drive_letter: 'J', reason: null };
    }
    return { entry_id, state: 'failed', presentation: null, drive_letter: null, reason: 'engine_unlock_failed' };
  },
  unmountEntry: async entry_id => {
    if (window.__allowFixtureUnmount) {
      window.__fixtureMounted = false;
      return { entry_id, state: 'unmounted', presentation: null, drive_letter: null, reason: null };
    }
    return { entry_id, state: 'failed', presentation: null, drive_letter: null, reason: 'dismount_failed' };
  },
  forgetPolicy: blocked
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
  await page.route('**/src/hooks/useBackend.ts*', route => route.fulfill({
    contentType: 'application/javascript', body: 'export default function useBackend(){return window.__vaultBackend;}'
  }));
  await page.route('**/src/utils/toast.ts*', route => route.fulfill({
    contentType: 'application/javascript', body: `
      window.__toasts = [];
      export const showSuccess = message => window.__toasts.push({ kind: 'success', message });
      export const showError = message => window.__toasts.push({ kind: 'error', message });
    `
  }));
  const source = await (await page.request.get(new URL('/src/panels/fleet/VaultAccessTab.tsx', origin).href)).text();
  const reactModule = source.match(/from "([^"]*\/react\.js[^"]*)"/)?.[1];
  assert.ok(reactModule, 'Resolve the same React instance as the real component');
  await page.route('**/__vault_selection_fixture.js', route => route.fulfill({ contentType: 'application/javascript', body: fixtureModule.replace('/node_modules/.vite/deps/react.js', reactModule) }));
  await page.route('**/__vault_selection__', route => route.fulfill({ contentType: 'text/html', body: fixture }));
  const openEditor = async () => {
    await page.getByRole('button', { name: 'Edit', exact: true }).click();
    await page.locator('.vault-access-editor').waitFor();
  };
  try {
    await page.goto(new URL('/__vault_selection__', origin).href);
    await page.locator('.fleet-vault-lifecycle').first().getByRole('button', { name: 'Mount', exact: true }).click();
    await page.evaluate(() => { window.__mountReason = 'pro_not_installed'; });
    await page.getByLabel('Vault password', { exact: true }).fill('fixture-only-password');
    await page.getByRole('button', { name: 'Mount Vault', exact: true }).click();
    const missingPro = page.getByRole('dialog').getByRole('alert').filter({ hasText: 'Pro module is not installed' });
    await missingPro.waitFor();
    assert.equal(await missingPro.evaluate(element => getComputedStyle(element).backgroundColor), 'rgb(255, 241, 242)', 'Mount errors use a readable light-red box');
    assert.equal((await missingPro.innerText()).includes('password'), false, 'Missing engine is not a password failure');
    await page.evaluate(() => { window.__mountReason = null; });
    await page.getByLabel('Vault password', { exact: true }).fill('fixture-only-password');
    await page.getByRole('button', { name: 'Mount Vault', exact: true }).click();
    await page.getByRole('dialog').getByRole('alert').filter({ hasText: 'password, PIM, or keyfiles' }).waitFor();
    assert.equal(await page.getByLabel('Vault password', { exact: true }).inputValue(), '', 'Failed mounts must clear the password');
    await page.evaluate(() => { window.__allowFixtureMount = true; window.__mountAckOnly = true; window.__toasts = []; });
    await page.getByLabel('Vault password', { exact: true }).fill('fixture-only-password');
    await page.getByRole('button', { name: 'Mount Vault', exact: true }).click();
    await page.getByRole('dialog').getByRole('alert').filter({ hasText: 'has not confirmed that this Vault is mounted' }).waitFor();
    assert.equal(await page.evaluate(() => window.__toasts.some(toast => toast.kind === 'success')), false, 'An unverified mount receipt cannot notify success');
    await page.evaluate(() => { window.__mountAckOnly = false; });
    await page.getByLabel('Vault password', { exact: true }).fill('fixture-only-password');
    await page.getByRole('button', { name: 'Mount Vault', exact: true }).click();
    await page.getByRole('dialog').waitFor({ state: 'detached' });
    await page.getByRole('button', { name: 'Open in File Explorer', exact: true }).click();
    assert.equal(await page.evaluate(() => window.__openedDrive), 'J', 'Open uses the authorized mounted drive');
    await page.getByRole('button', { name: 'Unmount', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: 'could not be safely unmounted' }).waitFor();
    assert.equal(await page.getByRole('button', { name: 'Open in File Explorer', exact: true }).count(), 1, 'Failed dismount does not pretend the drive disappeared');
    await page.evaluate(() => { window.__allowFixtureUnmount = true; window.__toasts = []; });
    await page.getByRole('button', { name: 'Unmount', exact: true }).click();
    await page.waitForFunction(() => window.__toasts.some(toast => toast.kind === 'success' && /unmounted/i.test(toast.message)));
    await page.getByRole('status').filter({ hasText: 'unmounted' }).waitFor();
    await page.getByRole('button', { name: 'Refresh', exact: true }).click();
    await page.locator('.fleet-vault-lifecycle').first().getByRole('button', { name: 'Mount', exact: true }).waitFor();
    // Another window mounted it after this page's last service observation.
    await page.evaluate(() => { window.__fixtureMounted = true; window.__reportAlreadyMounted = true; });
    await page.locator('.fleet-vault-lifecycle').first().getByRole('button', { name: 'Mount', exact: true }).click();
    await page.getByLabel('Vault password', { exact: true }).fill('fixture-only-password');
    await page.getByRole('button', { name: 'Mount Vault', exact: true }).click();
    await page.getByRole('dialog').waitFor({ state: 'detached' });
    await page.getByRole('alert').filter({ hasText: 'already mounted' }).waitFor();
    await page.getByRole('button', { name: 'Open in File Explorer', exact: true }).waitFor();
    await page.evaluate(() => { window.__fixtureMounted = false; window.__reportAlreadyMounted = false; });
    await page.getByRole('button', { name: 'Refresh', exact: true }).click();
    await openEditor();
    const preferredLetter = page.getByLabel('Vault 1 preferred drive letter', { exact: true });
    await preferredLetter.locator('option[value="J"]').waitFor({ state: 'attached' });
    assert.equal(await preferredLetter.evaluate(element => element.tagName), 'SELECT');
    assert.equal(await preferredLetter.locator('option[value="C"]').count(), 0, 'Occupied letters are not offered');
    assert.equal(await preferredLetter.locator('option[value="J"]').isEnabled(), true, 'Editing can retain its own reservation');
    await page.evaluate(() => { window.__occupiedLetters = ['W']; });
    await page.getByRole('button', { name: 'Refresh free letters', exact: true }).click();
    await page.waitForFunction(() => ![...document.querySelectorAll('select[aria-label="Vault 1 preferred drive letter"] option')].some(option => option.value === 'W'));
    await page.evaluate(() => { window.__occupiedLetters = []; });
    const owner = page.locator('.vault-access-editor select').first();
    for (const select of [owner, preferredLetter]) {
      assert.equal(await select.evaluate(element => getComputedStyle(element).borderTopWidth), '1px', 'Native dropdown has a visible boundary');
      assert.equal(await select.evaluate(element => getComputedStyle(element).borderTopColor), 'rgb(139, 157, 149)');
    }
    const labels = await owner.locator('option').allTextContents();
    assert.ok(labels.some(label => label.includes('ExampleUser') && /current user/i.test(label)));
    assert.ok(labels.every(label => !label.includes('S-1-5-')), 'Owner labels must not show SID numbers');
    await page.evaluate(() => { window.__policyError = 'vault_owner_required: forbidden after version conflict'; window.__toasts = []; });
    await page.getByRole('button', { name: 'Save vault settings', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: 'Ask its owner to make changes' }).waitFor();
    assert.equal(await page.evaluate(() => window.__toasts.some(toast => toast.kind === 'success')), false);
    await page.evaluate(() => { window.__policyError = null; window.__ignoreSave = true; window.__toasts = []; });
    await page.getByRole('button', { name: 'Save vault settings', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: 'has not confirmed' }).waitFor();
    assert.equal(await page.evaluate(() => window.__toasts.some(toast => toast.kind === 'success')), false, 'A no-op save receipt cannot notify success');
    await page.evaluate(() => { window.__ignoreSave = false; });
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
    await page.getByLabel('Vault 2 preferred drive letter', { exact: true }).selectOption('K');
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
    console.log('PASS: readable missing-Pro versus unlock errors; ownership-specific save failure; unverified save/mount do not notify success; confirmed dismount notification; bordered free-letter/owner dropdowns; password clearing and authorized Open; presets/drafts/removals persist through reload.');
  } catch (error) {
    console.error({ browserErrors: errors, fixtureText: await page.locator('body').innerText() });
    throw error;
  } finally {
    await browser.close();
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
