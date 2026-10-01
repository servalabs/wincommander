const { chromium } = require(process.env.WINCOMMANDER_PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');

// Browser bootstrap regression; this does not simulate Windows activation or RDS.
(async () => {
    const browser = await chromium.launch({ headless: true });
    try {
        const page = await browser.newPage();
        await page.addInitScript(() => {
            window.revealCalls = 0;
            window.__TAURI_INTERNALS__ = {
                metadata: { currentWindow: { label: 'main' } },
                invoke: command => {
                    if (command === 'get_setting') return Promise.resolve('dark');
                    if (command === 'startup_window_ready') window.revealCalls++;
                    return new Promise(() => {});
                },
                transformCallback: () => 1,
            };
        });
        const mainRequested = page.waitForRequest(request =>
            new URL(request.url()).pathname === '/src/main.tsx', { timeout: 15_000 });
        const started = Date.now();
        await page.goto(process.argv[2] || 'http://127.0.0.1:1438');
        await page.waitForFunction(() => window.revealCalls === 1);
        const otherPage = await browser.newPage();
        await otherPage.setContent('<label>Other application <input aria-label="Other application"></label>');
        await otherPage.bringToFront();
        await otherPage.getByRole('textbox').fill('Typing while the reveal reply stays pending');
        await mainRequested;
        const elapsedMs = Date.now() - started;
        const revealCalls = await page.evaluate(() => window.revealCalls);
        assert.equal(revealCalls, 1, 'A late reply must not spawn repeated reveal requests');
        assert.ok(elapsedMs < 15_000, 'Pending native reveal must not strand module loading');
        assert.equal(await otherPage.getByRole('textbox').inputValue(),
            'Typing while the reveal reply stays pending');
        console.log(JSON.stringify({ mainModuleRequested: true, revealCalls, elapsedMs,
            nativeRevealStillPending: true, evidence: 'browser bootstrap only' }));
    } finally {
        await browser.close();
    }
})().catch(error => { console.error(error); process.exit(1); });
