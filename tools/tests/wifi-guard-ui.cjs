// SPDX-License-Identifier: AGPL-3.0-or-later
// Real component, synthetic app state; never connects to Windows or changes Wi-Fi.
const { chromium } = require(process.env.WC_PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');
(async () => {
  const browser = await chromium.launch({ headless: true, executablePath: process.env.WC_BROWSER_EXE });
  try {
    const page = await browser.newPage({ viewport: { width: 900, height: 850 } });
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.route('**/src/context/AppContext.tsx*', route => route.fulfill({ contentType: 'application/javascript', body: `
      export function useAppState() { return { systemInfo: {isAdmin:true},
        appSettings: {ideal:{network:{wifiGuard:{enabled:true,baseline:[]}}}},
        patchAppSettings: async()=>{throw new Error('Settings mutation forbidden in fixture')},
        refreshSettings: async()=>{} }; }
      export function useOptionalAppState() { return useAppState(); }
    ` }));
    const url = `${process.env.WC_UI_BASE_URL || 'http://127.0.0.1:1420'}/tools/fixtures/wifi-guard-ui.html`;
    await page.goto(url, { waitUntil: 'domcontentloaded' });
    await page.getByText('Windows denied access to Wi-Fi details.', { exact: false }).waitFor();
    await page.getByText('Recent Wi-Fi alerts (0)', { exact: true }).waitFor();
    const help = [
      ['About Wi-Fi Guard', ['Checks the Wi-Fi connection', 'Last checked:', 'Turning this PC’s MAC randomization']],
      ['Wi-Fi reporting and notifications', ['Network names and access-point identities', 'Popup delivery follows']],
      ['About Wi-Fi alert history', ['No alerts recorded in this observer session.', 'Up to 100 alerts']],
    ];
    for (const [name, phrases] of help) {
      for (const phrase of phrases) assert.equal(await page.getByText(phrase, { exact: false }).count(), 0);
      const button = page.getByRole('button', { name, exact: true });
      await button.focus(); await page.keyboard.press('Enter');
      for (const phrase of phrases) await page.getByText(phrase, { exact: false }).waitFor();
      await page.keyboard.press('Escape');
      await page.waitForFunction(label => document.activeElement?.getAttribute('aria-label') === label, name);
    }
    await page.setViewportSize({ width: 390, height: 400 });
    await page.getByRole('button', { name: 'About Wi-Fi Guard', exact: true }).click();
    const bounds = await page.locator('[data-slot="popover-content"]').boundingBox();
    assert.ok(bounds.x >= 0 && bounds.x + bounds.width <= 391 && bounds.height <= 281);
    await page.keyboard.press('Escape');
    await page.goto(url + '?alerts', { waitUntil: 'domcontentloaded' });
    await page.getByText('Recent Wi-Fi alerts (1)', { exact: true }).waitFor();
    await page.getByText('Example network', { exact: false }).waitFor();
    await page.getByRole('button', { name: 'About Wi-Fi alert history', exact: true }).click();
    assert.equal(await page.getByText('No alerts recorded in this observer session.', { exact: false }).count(), 0);
    await page.keyboard.press('Escape');
    await page.goto(url + '?error', { waitUntil: 'domcontentloaded' });
    await page.getByRole('alert').filter({ hasText: 'Synthetic Wi-Fi read failure' }).waitFor();
    assert.deepEqual(errors, []);
    assert.ok(await page.evaluate(() => window.wifiFixtureCalls.every(command => ['wifi_guard_status', 'get_wifi_guard_recent', 'plugin:event|listen', 'plugin:event|unlisten'].includes(command))));
    console.log('PASS: Wi-Fi help hidden until opened; keyboard/Escape/focus; narrow popovers; alerts and errors remain visible.');
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
