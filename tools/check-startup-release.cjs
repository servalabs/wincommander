const { chromium } = require(process.env.WINCOMMANDER_PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { createHash } = require('node:crypto');

// Exercise built assets under the hash-bearing policy used by packaged Tauri.
// A development server alone does not enforce this policy.
(async () => {
    const dist = path.resolve(process.argv[2] || 'dist');
    const html = fs.readFileSync(path.join(dist, 'index.html'), 'utf8').replace(/\r\n/g, '\n');
    const entry = html.match(/<script\b[^>]*\bsrc="([^"]+)"/)?.[1];
    assert.ok(entry, 'Production HTML must reference its startup entry');
    const config = JSON.parse(fs.readFileSync('src-tauri/commander-free/tauri.conf.json', 'utf8'));
    let csp = config.app.security.csp;
    for (const [tag, directive] of [['script', 'script-src'], ['style', 'style-src']]) {
        const hashes = [...html.matchAll(new RegExp(`<${tag}\\b[^>]*>([\\s\\S]*?)</${tag}>`, 'g'))]
            .filter(match => match[1].trim())
            .map(match => `'sha256-${createHash('sha256').update(match[1]).digest('base64')}'`);
        csp = csp.replace(new RegExp(`(${directive}[^;]*)`), `$1 ${hashes.join(' ')}`);
    }
    const browser = await chromium.launch({ headless: true });
    try {
        for (const scenario of [
            { saved: 'dark', system: 'light', cached: null, expected: 'rgb(10, 15, 18)' },
            { saved: 'light', system: 'dark', cached: null, expected: 'rgb(255, 255, 255)' },
            { saved: 'system', system: 'light', cached: null, expected: 'rgb(255, 255, 255)' },
            { saved: 'system', system: 'dark', cached: null, expected: 'rgb(10, 15, 18)' },
            { saved: 'light', system: 'dark', cached: 'dark', expected: 'rgb(255, 255, 255)' },
            { saved: 'dark', system: 'light', cached: 'light', expected: 'rgb(10, 15, 18)' },
        ]) {
        const page = await browser.newPage();
        await page.emulateMedia({ colorScheme: scenario.system, reducedMotion: 'no-preference' });
        const violations = [];
        await page.exposeFunction('recordPolicyViolation', value => violations.push(value));
        await page.addInitScript(({ saved, cached }) => {
            if (cached) localStorage.setItem('wc-theme', cached);
            else localStorage.removeItem('wc-theme');
            document.addEventListener('securitypolicyviolation', event => {
                window.recordPolicyViolation({ directive: event.effectiveDirective, blocked: event.blockedURI,
                    source: event.sourceFile, line: event.lineNumber });
            });
            window.__TAURI_INTERNALS__ = {
                metadata: { currentWindow: { label: 'main' } },
                // Keep the dashboard pending while checking actual startup artwork.
                invoke: command => command === 'startup_window_ready'
                    ? Promise.resolve(true) : command === 'get_setting'
                        ? Promise.resolve(saved) : new Promise(() => {}),
                transformCallback: () => 1,
            };
        }, scenario);
        const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css',
            '.svg': 'image/svg+xml', '.png': 'image/png', '.woff2': 'font/woff2' };
        await page.route('http://127.0.0.1:1439/**', async route => {
            const pathname = decodeURIComponent(new URL(route.request().url()).pathname);
            const file = path.resolve(dist, `.${pathname === '/' ? '/index.html' : pathname}`);
            if (!file.startsWith(`${dist}${path.sep}`) || !fs.existsSync(file)) {
                await route.fulfill({ status: 404, body: '' });
                return;
            }
            await route.fulfill({ body: fs.readFileSync(file), headers: {
                'content-type': mime[path.extname(file)] || 'application/octet-stream',
                'content-security-policy': csp,
            } });
        });
        await page.goto('http://127.0.0.1:1439/');
        await page.waitForFunction(() => document.querySelector('#startup-animation')?.shadowRoot?.querySelector('.splash-screen'));
        await page.waitForTimeout(500);
        const presentation = await page.evaluate(() => {
            const shadow = document.querySelector('#startup-animation').shadowRoot;
            const splash = shadow.querySelector('.splash-screen');
            const ring = shadow.querySelector('.sp-ring-outer');
            const logo = shadow.querySelector('.sp-logo-img');
            return { position: getComputedStyle(splash).position,
                visible: !document.querySelector('#startup-animation').hidden,
                background: getComputedStyle(splash).backgroundColor,
                height: splash.getBoundingClientRect().height, viewportHeight: innerHeight,
                animation: getComputedStyle(ring).animationName,
                styleRules: shadow.querySelector('link[rel="stylesheet"]').sheet?.cssRules.length || 0,
                logoLoaded: logo.complete && logo.naturalWidth > 0 };
        });
        const advancing = await page.evaluate(async () => {
            const ring = document.querySelector('#startup-animation').shadowRoot.querySelector('.sp-ring-outer');
            const animation = ring.getAnimations()[0];
            const before = animation?.currentTime;
            await new Promise(resolve => setTimeout(resolve, 120));
            return Boolean(animation && animation.currentTime > before);
        });
        console.log(JSON.stringify({ scenario, presentation, advancing, violations }));
        assert.equal(presentation.position, 'fixed', 'Packaged startup artwork must fill the window');
        assert.equal(presentation.visible, true);
        assert.equal(presentation.background, scenario.expected);
        assert.equal(presentation.height, presentation.viewportHeight);
        assert.equal(presentation.animation, 'sp-spin');
        assert.equal(presentation.logoLoaded, true);
        assert.ok(presentation.styleRules > 0, 'Bundled splash CSS must be accepted by CSP');
        assert.equal(advancing, true);
        // App toast styling has a separate inline-CSS path; keep its violations
        // visible above without conflating them with the startup asset regression.
        const startupEntry = new URL(entry, page.url()).href;
        assert.equal(violations.filter(item => item.source === startupEntry).length, 0);
        await page.close();
        }
    } finally {
        await browser.close();
    }
})().catch(error => { console.error(error); process.exit(1); });
