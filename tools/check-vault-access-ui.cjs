// Run against a loopback Vite server. Fixtures contain no native IPC,
// credentials, actual Vaults, screenshots, or private paths.
const assert = require('node:assert/strict');
const { chromium } = require(process.env.WINCOMMANDER_PLAYWRIGHT_MODULE || 'playwright');
const origin = new URL(process.argv[2] || 'http://127.0.0.1:5173');
if (!['127.0.0.1', 'localhost', '[::1]'].includes(origin.hostname)) {
  throw new Error('Vault UI fixtures require a loopback server');
}
const fixture = `<!doctype html><html class="dark"><head>
<meta name="viewport" content="width=device-width,initial-scale=1">
<script type="module">
import RefreshRuntime from '/@react-refresh';
RefreshRuntime.injectIntoGlobalHook(window);
window.$RefreshReg$ = () => {};
window.$RefreshSig$ = () => type => type;
window.__vite_plugin_react_preamble_installed__ = true;
</script></head><body><div id="fixture"></div></body></html>`;

async function main() {
  const channel = process.env.WINCOMMANDER_PLAYWRIGHT_CHANNEL;
  const executablePath = process.env.WINCOMMANDER_PLAYWRIGHT_EXECUTABLE_PATH;
  const browser = await chromium.launch({
    headless: true,
    // CI may use a downloaded Chromium build. Local Windows verification can
    // explicitly use an already-installed browser without creating a profile.
    ...(channel ? { channel } : {}),
    ...(executablePath ? { executablePath } : {}),
  });
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  page.setDefaultTimeout(15000);
  const browserErrors = [];
  page.on('pageerror', error => browserErrors.push(error.name));
  await page.route('**/src/hooks/useBackend.ts*', route => route.fulfill({
    contentType: 'application/javascript', body: 'export default function useBackend(){return window.__vaultBackend;}'
  }));
  await page.route('**/src/hooks/useVaultAccess.ts*', route => route.fulfill({
    contentType: 'application/javascript', body: 'export const FLEET_VAULTS_CHANGED_EVENT = "fleet-vaults-changed"; export default function useVaultAccess(){return window.__vaultUiService;}'
  }));
  await page.route('**/__vault_access_ui__', route => route.fulfill({ contentType: 'text/html', body: fixture }));
  const reset = async state => {
    await page.evaluate(value => window.renderVaultFixture(value), state);
    if (state === 'unelevated') {
      await page.getByRole('alert').filter({ hasText: 'Vault administrator access was not confirmed' }).waitFor();
    } else if (state === 'unavailable' || state === 'standard-owner-policy-unavailable') {
      await page.getByRole('alert').filter({ hasText: 'Vault settings could not be loaded yet' }).waitFor();
    } else if (state === 'outsider' || state === 'mounted') {
      await page.getByRole('button', { name: 'Edit', exact: true }).waitFor();
    } else if (state.startsWith('standard-owner')) {
      await page.locator('.vault-access-editor').waitFor();
    } else {
      // Saved policies intentionally start collapsed. The fixture must follow
      // the real explicit-edit workflow, not force the product editor open.
      const edit = page.getByRole('button', { name: 'Edit', exact: true }).first();
      await edit.waitFor();
      assert.equal(await page.locator('.vault-access-editor').count(), 0, 'Saved policy editor starts closed');
      await edit.click();
      await page.locator('.vault-access-editor').waitFor();
    }
    await page.getByText('Loading your Vault access…').waitFor({ state: 'hidden' });
  };
  const closed = () => page.locator('.vault-access-details').evaluate(element => !element.open);
  try {
    await page.goto(new URL('/__vault_access_ui__', origin).href);
    await page.waitForFunction(() => window.__vite_plugin_react_preamble_installed__ === true);
    await page.evaluate(async () => {
      const fixtureModule = await import('/tools/fixtures/vault-access-ui.js');
      window.renderVaultFixture = fixtureModule.renderVaultFixture;
    });
    await reset('saved');
    console.log('Vault UI fixture rendered.');
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
    const ownerPicker = page.getByLabel('Vault 1 primary owner', { exact: true });
    assert.ok((await ownerPicker.locator('option').allTextContents()).some(text => text.includes('Example standard user')), 'Enabled standard users remain selectable as owners');
    assert.ok((await ownerPicker.locator('option').allTextContents()).some(text => text.includes('Example administrator')), 'Enabled administrators remain selectable as owners');
    await page.locator('[data-vault-access-preset="shared-write"]').click();
    assert.equal(await page.locator('[data-vault-access-preset="shared-write"]').getAttribute('aria-checked'), 'true', 'Third permission choice remains shared read/write');
    assert.equal(await page.locator('[data-vault-access-preset="shared-read"]').getAttribute('aria-checked'), 'false', 'Third permission choice does not collapse to view-only');
    console.log('Vault UI PASS: disclosure, hover/focus/Escape, keyboard and named controls.');

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
    console.log('Vault UI PASS: 1440px, 720px and 360px geometry; no nested editor scrolling.');

    await page.setViewportSize({ width: 1440, height: 900 });
    for (const [state, text] of [
      ['saved', 'Showing the policy saved by the security service.'],
      ['draft', 'Draft auto-saved on this PC — not yet applied to Windows.'],
      ['unauthorized', 'This Windows account is not authorized to mount this vault.']
    ]) {
      await reset(state);
      assert.equal(await closed(), true);
      await page.getByText(text, { exact: true }).waitFor();
      assert.equal(await page.getByText(text, { exact: true }).isVisible(), true, `${state} message remains visible`);
      console.log(`Vault UI PASS: ${state} state.`);
    }
    await reset('dual');
    await page.locator('.fleet-vault-workspace').getByRole('button', { name: 'Mount', exact: true }).click();
    await page.getByRole('dialog').waitFor();
    assert.equal(await page.getByLabel('Hidden volume protection password', { exact: true }).isVisible(), true, 'Writable outer mount requests hidden protection');
    await page.keyboard.press('Escape');
    await page.getByRole('dialog').waitFor({ state: 'hidden' });

    await reset('apply-sibling');
    await page.getByRole('button', { name: 'Add private vault', exact: true }).click();
    const newEditor = page.locator('.vault-access-editor').last();
    await newEditor.locator('input[aria-label$="container path"]').fill('C:\\Vaults\\new-free.vc');
    await newEditor.locator('select[aria-label$="preferred drive letter"]').selectOption('K');
    await newEditor.locator('select[aria-label$="primary owner"]').selectOption('S-1-5-21-fixture-standard');
    await page.evaluate(() => window.__fixtureResetVaultTelemetry());
    await page.getByRole('button', { name: 'Save vault settings', exact: true }).click();
    await page.waitForFunction(() => window.__fixtureVaultTelemetry.appliedFragments.length === 1);
    const applyTelemetry = await page.evaluate(() => window.__fixtureVaultTelemetry);
    assert.equal(applyTelemetry.appliedFragments[0].entries.length, 1, 'Only the new Vault is submitted');
    assert.equal(applyTelemetry.appliedFragments[0].entries[0].entry.mount.preferred_letter, 'K');
    assert.equal(applyTelemetry.appliedFragments[0].entries[0].entry.primary_owner_sid, 'S-1-5-21-fixture-standard', 'Administrator can assign a new private Vault directly to a selectable Windows user');
    assert.deepEqual(applyTelemetry.driveLetterRequests, [applyTelemetry.appliedFragments[0].entries[0].entry.id], 'Only the new Vault receives save-time availability preflight');
    assert.equal(await page.getByText('This drive letter is occupied or reserved. Choose another free letter before saving.').count(), 0, 'Mounted sibling does not produce a false drive-letter banner');
    const assignedEntry = await page.evaluate(() => window.__fixtureLatestFragment.entries.find(row => row.entry.id === window.__fixtureVaultTelemetry.appliedFragments[0].entries[0].entry.id));
    assert.equal(assignedEntry.can_edit_policy, false, 'Service projection revokes administrator edit capability after assignment to another owner');
    console.log('Vault UI PASS: mounted same-owner sibling does not block a new free Vault save.');

    await reset('standard-owner');
    const standardEditor = page.locator('.vault-access-editor');
    await standardEditor.waitFor();
    const standardOwnerPicker = standardEditor.locator('select[aria-label$="primary owner"]');
    assert.equal(await standardOwnerPicker.isDisabled(), true, 'Standard owner cannot transfer a saved private Vault');
    assert.equal((await standardOwnerPicker.locator('option').allTextContents()).some(text => text.includes('Example administrator')), false, 'Standard owner never receives the Windows owner directory');
    await standardEditor.locator('input[aria-label$="label"]').fill('Standard owner update');
    await page.evaluate(() => window.__fixtureResetVaultTelemetry());
    await page.getByRole('button', { name: 'Save vault settings', exact: true }).click();
    await page.waitForFunction(() => window.__fixtureVaultTelemetry.appliedFragments.length === 1);
    const standardTelemetry = await page.evaluate(() => window.__fixtureVaultTelemetry);
    assert.equal(standardTelemetry.appliedFragments[0].entries[0].entry.label, 'Standard owner update', 'Standard owner can save an allowed own update');
    console.log('Vault UI PASS: standard owner edits own private Vault without an ownership-transfer path.');
    await reset('standard-owner-degraded');
    assert.equal(await page.getByRole('alert').filter({ hasText: 'Mounting is unavailable until this is fixed' }).isVisible(), true, 'Standard owner sees their Vault degradation warning');
    await reset('standard-owner-status-unavailable');
    assert.equal(await page.getByRole('alert').filter({ hasText: 'Vault mount status could not be confirmed' }).isVisible(), true, 'Standard owner sees why their saved controls are locked after a status read failure');
    await reset('standard-owner-policy-unavailable');
    assert.equal(await page.getByRole('alert').filter({ hasText: 'Vault settings could not be loaded yet' }).isVisible(), true, 'Standard owner sees why the caller-filtered policy editor is unavailable');
    await reset('degraded');
    assert.equal(await closed(), true);
    assert.equal(await page.getByRole('alert').filter({ hasText: 'Mounting is unavailable until this is fixed' }).isVisible(), true, 'Degraded warning stays visible');
    assert.equal(await page.locator('.fleet-vault-workspace').getByRole('button', { name: 'Mount', exact: true }).isDisabled(), true);
    await reset('unelevated');
    assert.equal(await page.getByRole('alert').filter({ hasText: 'Vault administrator access was not confirmed' }).isVisible(), true);
    await reset('unavailable');
    assert.equal(await page.getByRole('alert').filter({ hasText: 'Vault settings could not be loaded yet' }).isVisible(), true);
    await reset('mounted');
    assert.equal(await page.getByRole('cell', { name: 'Mounted', exact: true }).isVisible(), true, 'Mounted state is displayed in the saved-Vault table');
    assert.equal(await page.getByRole('button', { name: 'Remove policy', exact: true }).isDisabled(), true, 'A mounted Vault remains policy-locked');
    await reset('outsider');
    assert.equal(await page.getByRole('button', { name: 'Edit', exact: true }).isDisabled(), true, 'Outsider cannot edit a saved Vault');
    assert.equal(await page.getByRole('button', { name: 'Manage access', exact: true }).isDisabled(), true, 'Outsider cannot manage access');
    assert.equal(await page.getByRole('button', { name: 'Remove policy', exact: true }).isDisabled(), false, 'Authorized outsider can remove only an unmounted policy');
    await reset('saved');
    await page.getByLabel('Grant 1 access', { exact: true }).selectOption('read');
    await page.evaluate(() => { window.__fixtureSetMounted(true); window.dispatchEvent(new Event('fleet-vaults-changed')); });
    await page.getByRole('button', { name: 'Edit', exact: true }).waitFor({ state: 'visible' });
    await page.waitForFunction(() => document.querySelector('[data-vault-editor-mode] button[disabled]') !== null);
    assert.equal(await page.getByText('Draft auto-saved on this PC — not yet applied to Windows.').isVisible(), true, 'Event status refresh preserves the dirty draft');
    await reset('saved');
    assert.equal(await page.locator('.fleet-validation-errors').isVisible(), true, 'Required-field validation is not hidden in help');
    const mount = page.locator('.fleet-vault-workspace').getByRole('button', { name: 'Mount', exact: true });
    await mount.click();
    await page.getByRole('dialog').waitFor();
    await page.keyboard.press('Escape');
    await page.getByRole('dialog').waitFor({ state: 'hidden' });
    assert.deepEqual(browserErrors, [], 'Fixture must not produce browser exceptions');
    console.log('Vault UI PASS: visible warnings, saved/draft/unauthorized/degraded/mounted states, modal Escape.');
  } finally {
    await browser.close();
  }
}
main().catch(error => { console.error(error.message); process.exitCode = 1; });
