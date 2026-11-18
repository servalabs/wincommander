// Loopback UI evidence only. Backend receipts are simulated; no files upload.
const assert = require('node:assert/strict');
const { chromium } = require(process.env.WINCOMMANDER_PLAYWRIGHT_MODULE || 'playwright');
const origin = new URL(process.argv[2] || 'http://127.0.0.1:1420');
if (!['127.0.0.1', 'localhost', '[::1]'].includes(origin.hostname)) throw new Error('Cloud backup fixture requires a loopback server');
const fixture = `<!doctype html><html class="dark"><head><meta name="viewport" content="width=device-width,initial-scale=1"><script type="module">
import RefreshRuntime from '/@react-refresh'; RefreshRuntime.injectIntoGlobalHook(window);
window.$RefreshReg$=()=>{}; window.$RefreshSig$=()=>type=>type; window.__vite_plugin_react_preamble_installed__=true;
</script></head><body><div id="fixture"></div></body></html>`;
(async () => {
  const browser = await chromium.launch({ headless: true,
    ...(process.env.WINCOMMANDER_BROWSER_EXECUTABLE ? { executablePath: process.env.WINCOMMANDER_BROWSER_EXECUTABLE } : {}),
  });
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  page.setDefaultTimeout(15000);
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.route('**/src/hooks/useBackend.ts*', route => route.fulfill({ contentType: 'application/javascript', body: 'export default function useBackend(){return window.__cloudBackend;} export async function executeBackendCommand(){throw new Error("Native commands are unavailable in this fixture");}' }));
  await page.route('**/__vault_cloud_backup_ui__', route => route.fulfill({ contentType: 'text/html', body: fixture }));
  const reset = async state => {
    await page.evaluate(async value => {
      const module = await import('/tools/fixtures/vault-cloud-backup-ui.js');
      module.renderCloudBackupFixture(value);
    }, state);
    await page.getByRole('heading', { name: 'Connection', exact: true }).waitFor();
  };
  const requests = () => page.evaluate(() => window.__cloudRequests);
  try {
    await page.goto(new URL('/__vault_cloud_backup_ui__', origin).href);
    await reset('new');
    assert.deepEqual((await requests()).map(request => request.action), ['status'], 'Opening setup only observes status');
    await page.getByRole('button', { name: 'Set up backup tool', exact: true }).click();
    await page.getByRole('button', { name: 'Connect Proton Drive', exact: true }).click();
    const save = page.getByRole('button', { name: 'Save backup setup', exact: true });
    assert.equal(await save.isDisabled(), true, 'Uploading needs explicit consent');
    await page.getByLabel('Choose files or folders', { exact: true }).check();
    await page.getByLabel('Selected paths inside this Vault — one per line', { exact: true }).fill('../Private');
    await page.getByLabel('I agree to copy these files to my Proton Drive account.', { exact: true }).check();
    await save.click();
    await page.getByRole('alert').filter({ hasText: 'Choose 1–32 paths inside this Vault' }).waitFor();
    assert.equal((await requests()).filter(request => request.action === 'configure').length, 0);
    await page.getByLabel('Selected paths inside this Vault — one per line', { exact: true }).fill('Documents\nPhotos');
    await save.click();
    await page.getByRole('status').filter({ hasText: 'Watching for changes' }).waitFor();
    const configured = (await requests()).find(request => request.action === 'configure');
    assert.equal(configured.configuration.automatic, true);
    assert.deepEqual(configured.configuration.source_relative_paths, ['Documents', 'Photos']);
    assert.equal(configured.expected_mount_receipt, 'a'.repeat(64));

    await reset('connected');
    await page.getByRole('status').filter({ hasText: 'Watching for changes' }).waitFor();
    await page.getByRole('dialog').evaluate(async element => { await Promise.all(element.getAnimations().map(animation => animation.finished)); });
    if (process.env.WINCOMMANDER_CLOUD_BACKUP_SCREENSHOT) await page.screenshot({ path: process.env.WINCOMMANDER_CLOUD_BACKUP_SCREENSHOT, fullPage: true });
    await page.getByRole('button', { name: 'Refresh', exact: true }).click();
    assert.equal((await requests()).some(request => request.action === 'connect'), false, 'Remount with saved session must not log in');
    assert.equal(await page.getByRole('button', { name: 'Connect Proton Drive', exact: true }).count(), 0);
    await page.getByRole('button', { name: 'Stop backup', exact: true }).click();
    await page.getByRole('status').filter({ hasText: 'Automatic backup is paused' }).waitFor();
    await page.getByText('Connected', { exact: true }).waitFor();
    assert.equal((await requests()).some(request => request.action === 'disconnect'), false, 'Stopping preserves the session');

    await reset('partial');
    await page.getByText('Backup partly finished', { exact: true }).waitFor();
    assert.equal(await page.getByText('Operation complete', { exact: true }).count(), 0, 'Partial is not complete');
    await page.evaluate(() => { window.__cloudState.mount_receipt = 'b'.repeat(64); });
    await page.getByRole('button', { name: 'Refresh', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: 'mounted identity changed' }).waitFor();
    assert.equal(await page.getByRole('button', { name: 'Back up now', exact: true }).count(), 0, 'Stale identity removes mutation controls');

    assert.deepEqual(errors, []);
    console.log('Cloud backup UI passed: explicit setup/consent, path rejection, automatic preference, retained session, Stop without logout, partial result and stale-mount fencing. Simulated backend only.');
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
