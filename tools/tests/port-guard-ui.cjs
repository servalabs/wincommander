// SPDX-License-Identifier: AGPL-3.0-or-later
// Run against a local Vite server. Requires Playwright; never connects to a native bridge.
const { chromium } = require(process.env.WC_PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');
(async () => {
  let browser;
  try {
    console.log('Launching isolated browser');
    browser = await chromium.launch({headless:true, executablePath:process.env.WC_BROWSER_EXE, timeout:15000});
    const page = await browser.newPage({viewport:{width:1100,height:900}});
    const errors=[];
    page.on('pageerror',error=>{ errors.push(error.message); console.log('PAGE ERROR',error.message); });
    console.log('Navigating');
    await page.goto('http://127.0.0.1:1420/tools/fixtures/port-guard-ui.html',{timeout:30000,waitUntil:'domcontentloaded'});
    await page.getByRole('table',{name:'Watched ports'}).waitFor({timeout:20000});
    await page.getByRole('table',{name:'Watched ports'}).locator('tbody tr').first().waitFor();
    assert.equal(await page.getByText('Last checked:',{exact:false}).count(),0);
    await page.getByRole('button',{name:'Port Guard coverage and diagnostics'}).click();
    await page.getByText('Limited coverage means the monitor is running', {exact:false}).waitFor();
    await page.keyboard.press('Escape');
    await page.waitForFunction(()=>document.activeElement?.getAttribute('aria-label')==='Port Guard coverage and diagnostics');
    assert.equal(await page.getByRole('button',{name:'Port Guard coverage and diagnostics'}).evaluate(node=>node===document.activeElement),true);
    await page.getByRole('button',{name:'Choosing a port preset'}).focus();
    await page.keyboard.press('Enter');
    await page.getByText('Choose a port on this PC, then select Add watch.',{exact:false}).waitFor();
    await page.keyboard.press('Escape');
    await page.getByRole('button',{name:/^AnyDesk direct 7070/}).click();
    assert.equal(await page.getByLabel('Ports to watch',{exact:true}).inputValue(),'7070');
    assert.equal(await page.getByLabel('Watch protocol',{exact:true}).inputValue(),'tcp');
    const presets=page.getByRole('region',{name:'Port presets'});
    assert.equal(await presets.getByRole('button',{name:/outbound reference|AnyDesk relay|TeamViewer service|Teams service|Zoom service|FTP active data source|PlayStation service|Epic-Games service|Battle.net service/i}).count(),0);
    assert.equal(await presets.getByText('Meetings',{exact:true}).count(),0);
    assert.equal(await page.getByRole('table',{name:'Watched ports'}).locator('tbody tr').count(),3);
    const history=page.getByRole('table',{name:'Grouped inbound observations'});
    assert.equal(await history.locator('tbody tr').count(),2);
    await history.getByText('4 observations',{exact:true}).click();
    assert.equal(await history.locator('details[open] time').count(),4);
    await page.getByText('Firewall port blocks',{exact:true}).first().click();
    await page.getByRole('table',{name:'Firewall port blocks'}).waitFor();
    const layouts=[];
    for (const width of [1100,760,390]) {
      await page.setViewportSize({width,height:900});
      const result=await page.evaluate(()=>({width:innerWidth, pageWidth:document.documentElement.scrollWidth,
        spills:[...document.querySelectorAll('.port-guard-table th,.port-guard-table td')].filter(cell=>cell.scrollWidth>cell.clientWidth+2).map(cell=>cell.textContent)}));
      layouts.push(result); assert.ok(result.pageWidth<=result.width+2,JSON.stringify(result)); assert.deepEqual(result.spills,[]);
    }
    await page.setViewportSize({width:390,height:400});
    await page.getByRole('button',{name:'Port Guard coverage and diagnostics'}).click();
    const help=page.locator('[data-slot="popover-content"]');
    await help.waitFor();
    const helpBounds=await help.boundingBox();
    assert.ok(helpBounds.x>=0 && helpBounds.x+helpBounds.width<=391);
    assert.ok(helpBounds.height<=281);
    await page.keyboard.press('Escape');
    await page.setViewportSize({width:1100,height:900});
    if (process.env.WC_UI_SCREENSHOT) await page.screenshot({path:process.env.WC_UI_SCREENSHOT,fullPage:true});
    await page.getByLabel('Firewall ports',{exact:true}).fill('65536');
    await page.getByLabel('Firewall label',{exact:true}).fill('FixtureTest');
    await page.getByRole('button',{name:'Block port',exact:true}).click();
    await page.getByRole('alert').filter({hasText:'65535'}).waitFor();
    assert.equal(await page.evaluate(()=>window.guardFixtureCalls.filter(call=>call.args.command==='Block-Protocol').length),0);
    await page.getByLabel('Firewall ports',{exact:true}).fill('49171');
    await page.getByLabel('Firewall label',{exact:true}).fill('invalid label');
    await page.getByRole('button',{name:'Block port',exact:true}).click();
    await page.getByRole('alert').filter({hasText:'underscores or hyphens'}).waitFor();
    assert.equal(await page.evaluate(()=>window.guardFixtureCalls.filter(call=>call.args.command==='Block-Protocol').length),0);
    await page.getByLabel('Firewall label',{exact:true}).fill('FixtureTest');
    await page.getByRole('button',{name:'Block port',exact:true}).click();
    await page.getByRole('alert').filter({hasText:'Synthetic creation failed verification'}).waitFor();
    await page.getByRole('cell',{name:/Needs review/}).waitFor();
    assert.equal(await page.getByLabel('Firewall ports',{exact:true}).inputValue(),'49171');
    assert.ok(await page.evaluate(()=>window.guardFixtureCalls.filter(call=>call.args.command==='Get-ProtocolBlocks').length)>=3);
    assert.deepEqual(errors,[]);
    console.log(JSON.stringify({passed:true,layouts,errors}));
  } finally { await browser?.close(); }
})().catch(error=>{console.error(error);process.exitCode=1;});
