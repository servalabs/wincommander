// Real components with synthetic state and an intercepted native bridge.
const { chromium } = require(process.env.WC_PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');

(async () => {
  const browser = await chromium.launch({ headless: true, executablePath: process.env.WC_BROWSER_EXE });
  try {
    const page = await browser.newPage({ viewport: { width: 1100, height: 900 } });
    const errors = [];
    page.on('pageerror', error => { errors.push(error.message); console.error('PAGE ERROR', error.message); });
    page.on('console', message => { if (message.type() === 'error') console.error('BROWSER', message.text()); });
    await page.route('**/src/context/AppContext.tsx*', route => route.fulfill({ contentType:'application/javascript', body:`
      export function useAppState() { return { appSettings: { ideal: { identity: { advancedToolsEnabled:location.search.includes('investigator') } } } }; }
      export function useOptionalAppState() { return useAppState(); }
    ` }));
    const url = `${process.env.WC_UI_BASE_URL || 'http://127.0.0.1:1425'}/tools/fixtures/selected-controls-ui.html`;
    await page.goto(url, { waitUntil: 'domcontentloaded' });
    const card = title => page.locator('.section-card').filter({hasText:title});
    const update = card('Windows Update restarts');
    await update.getByRole('button', { name:'Set schedule and prevent restart' }).waitFor();
    await update.getByRole('button', { name:'Set schedule and prevent restart' }).click();
    await page.getByText('Windows requires scheduled update installation', {exact:false}).waitFor();
    await page.getByRole('button', {name:'Back', exact:true}).click();
    assert.equal(await page.evaluate(() => window.selectedCalls.filter(call => call.command === 'Set-UpdateRestartPolicy').length), 0);
    await update.getByRole('button', { name:'Set schedule and prevent restart' }).click();
    await page.getByRole('button', { name:'Set schedule and prevent restart', exact:true }).last().click();
    await update.getByText('The policy is set by WinCommander.').waitFor();
    await update.getByRole('button', {name:'Restore previous setting'}).click();
    await update.getByText('This policy is not set.').waitFor();

    const shell = card('Start and Settings visibility');
    const start = shell.locator('div.flex.flex-wrap.items-center.justify-between').filter({hasText:'Start recommended section'});
    await start.getByRole('button', {name:'Hide'}).click();
    await start.getByText('Hide policy set by WinCommander').waitFor();
    await start.getByRole('button', {name:'Restore'}).click();
    await start.getByText('Not hidden by WinCommander').waitFor();

    const context = card('Right-click menu items');
    const share = context.locator('div.flex.flex-wrap.items-center.justify-between').filter({hasText:'Share'});
    await share.getByRole('button', {name:'Hide'}).click();
    await share.getByText('Registration hidden by WinCommander').waitFor();
    await share.getByRole('button', {name:'Restore'}).click();
    await share.getByText('Registration detected').waitFor();

    const permissions = card('More app-permission defaults');
    const bluetooth = permissions.getByRole('row').filter({hasText:'Bluetooth devices'});
    await bluetooth.getByRole('button', {name:'Block default'}).first().click();
    await bluetooth.getByText('Default set to block').waitFor();
    await bluetooth.getByRole('button', {name:'Restore'}).click();
    assert.equal(await bluetooth.getByText('Default allows').count(), 2);

    const ip = card('IP firewall rules');
    await ip.getByLabel('IP address or subnet').fill('bad');
    await ip.getByRole('button', {name:'Add rule'}).click();
    await page.getByRole('button', {name:'Block IP'}).click();
    await ip.getByRole('alert').filter({hasText:'Invalid IP address'}).waitFor();
    await ip.getByLabel('IP address or subnet').fill('192.0.2.12');
    await ip.getByRole('button', {name:'Add rule'}).click();
    await page.getByRole('button', {name:'Block IP'}).click();
    await ip.getByText('192.0.2.12').waitFor();
    await ip.getByRole('button', {name:'Remove Block 192.0.2.12 Outbound rule'}).click();
    await ip.getByText('No WinCommander IP rules.').waitFor();

    await page.getByRole('button', {name:'Copy history report'}).click();
    const report = await page.evaluate(() => window.copiedReport);
    assert.match(report, /not a complete network history or packet capture/);
    assert.match(report, /queue losses: 2; older rows expired: 1/);
    assert.match(report, /192\.0\.2\.2:10/);
    await page.getByText('Sampled endpoint report', {exact:true}).click();
    const sampling = page.locator('.active-ports-history');
    await sampling.getByText('Recording off.', {exact:false}).waitFor();
    assert.equal(await sampling.getByRole('button', {name:'Copy session report'}).isDisabled(), true);
    await sampling.getByRole('button', {name:'Start recording'}).click();
    await sampling.getByText('1 rows retained', {exact:false}).waitFor({timeout:15000});
    await sampling.getByRole('button', {name:'Copy session report'}).click();
    const sessionReport = await page.evaluate(() => window.copiedReport);
    assert.match(sessionReport, /198\.51\.100\.4/);
    assert.match(sessionReport, /not a complete traffic log/);
    assert.doesNotMatch(sessionReport, /private/);
    await page.getByText('Pause Port Monitoring', {exact:true}).click();
    await sampling.getByText('Recording paused with the view.', {exact:false}).waitFor();
    await sampling.getByRole('button', {name:'Copy session report'}).click();
    assert.match(await page.evaluate(() => window.copiedReport), /Recording: paused/);
    await page.getByText('Pause Port Monitoring', {exact:true}).click();
    await sampling.getByText('Recording samples.', {exact:false}).waitFor();
    await sampling.getByRole('button', {name:'Stop recording'}).click();
    await sampling.getByText('Recording off.', {exact:false}).waitFor();
    await sampling.getByRole('button', {name:'Clear and stop'}).click();
    assert.equal(await sampling.getByRole('button', {name:'Copy session report'}).isDisabled(), true);
    for (const width of [1100, 760, 390]) {
      await page.setViewportSize({ width, height:900 });
      const layout = await page.evaluate(() => ({width:innerWidth,content:document.documentElement.scrollWidth}));
      assert.ok(layout.content <= layout.width + 2, JSON.stringify(layout));
    }
    await page.goto(url + '?limited&investigator', { waitUntil:'domcontentloaded' });
    await page.getByText('Not supported on this edition or version').waitFor();
    await page.getByText('Controlled by an account policy').waitFor();
    await page.getByText('Changed elsewhere — review needed').waitFor();
    await page.getByText('Registration not detected').waitFor();
    const unavailable = card('More app-permission defaults').getByRole('row').filter({hasText:'Presence sensing'});
    assert.equal(await unavailable.getByText('Not detected on this PC').count(), 2);
    assert.equal(await unavailable.getByRole('button', {name:'Block default'}).first().isDisabled(), true);
    assert.equal(await card('Windows Update restarts').getByRole('button', {name:'Set schedule and prevent restart'}).isDisabled(), true);
    assert.equal(await card('IP firewall rules').getByRole('button', {name:'Add rule'}).isDisabled(), true);
    assert.equal(await page.evaluate(() => window.selectedCalls.filter(call => /^(Set-|Add-|Remove-)/.test(call.command)).length), 0);
    await page.goto(url + '?investigator', { waitUntil:'domcontentloaded' });
    const investigatorUpdate = card('Windows Update restarts');
    await investigatorUpdate.getByText('Changes are unavailable while Investigator mode is active.').waitFor();
    assert.equal(await investigatorUpdate.getByRole('button', {name:'Set schedule and prevent restart'}).isDisabled(), true);
    assert.equal(await card('IP firewall rules').getByRole('button', {name:'Add rule'}).isDisabled(), true);
    await page.goto(url + '?pending', { waitUntil:'domcontentloaded' });
    const pendingStart = card('Start and Settings visibility').locator('div.flex.flex-wrap.items-center.justify-between').filter({hasText:'Start recommended section'});
    await pendingStart.getByRole('button', {name:'Cancel change'}).click();
    await pendingStart.getByText('Not hidden by WinCommander').waitFor();
    const pendingBluetooth = card('More app-permission defaults').getByRole('row').filter({hasText:'Bluetooth devices'});
    await pendingBluetooth.getByRole('button', {name:'Cancel change'}).click();
    assert.equal(await pendingBluetooth.getByText('Default allows').count(), 2);
    assert.equal(await page.evaluate(() => window.selectedCalls.filter(call => /^(Set-ShellVisibility|Set-AdditionalAppPermission)$/.test(call.command) && call.params.Mode === 'restore').length), 2);
    assert.deepEqual(errors, []);
    console.log('PASS: synthetic controls apply, restore, preserve confirmation, show errors, export history and fit narrow screens.');
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
