import { expect, test } from 'bun:test';

declare const Bun: {
    which(name: string): string | null;
    spawn(args: string[], options: { stdout: 'pipe'; stderr: 'pipe' }): {
        stderr: ReadableStream<Uint8Array>;
        exited: Promise<number>;
    };
};

test('bootstrap handoff keeps one animation root and ignores stale effect cleanup', async () => {
    // Isolate module mocks from the rest of the frontend suite.
    const script = `
        import { mock } from 'bun:test';
        import { strict as assert } from 'node:assert';
        let roots = 0;
        const frames = [];
        const links = [];
        let decoded = false;
        const shadow = { append() {}, querySelector: selector => selector === '.sp-logo-img'
            ? { decode: async () => { decoded = true; } } : {} };
        const node = { hidden: false, shadowRoot: shadow, attachShadow: () => shadow };
        mock.module('react-dom/client', () => ({ createRoot: () => {
            roots++;
            return { render: value => frames.push(value) };
        } }));
        mock.module('./src/components/StartupAnimation', () => ({ default: () => null }));
        mock.module('./src/components/SplashScreen.css?url', () => ({ default: '/assets/splash.css' }));
        globalThis.document = { createElement: tag => {
            if (tag === 'link') { const link = {}; links.push(link); return link; }
            assert.notEqual(tag, 'style');
            return node;
        }, body: { append() {} }, fonts: { load: async () => [], ready: Promise.resolve() } };
        globalThis.requestAnimationFrame = callback => { callback(); return 1; };
        const { showStartupAnimation: show, hideStartupAnimation: hide,
            waitForStartupAnimationReady: ready } = await import('./src/startup/animationRoot');
        const initial = { branding: { companyLabel: 'SERVAlABS', productLabel: 'WINCOMMANDER' },
            isLight: false, reducedMotion: false, isAppReady: false,
            startupError: null, onComplete() {}, onRetry() {} };
        show(initial);
        assert.equal(links.length, 1);
        assert.equal(links[0].rel, 'stylesheet');
        assert.equal(links[0].href, '/assets/splash.css');
        assert.equal(frames.length, 0);
        let painted = false;
        const readyPromise = ready().then(() => { painted = true; });
        await Promise.resolve();
        assert.equal(painted, false);
        assert.equal(decoded, false);
        links[0].onload();
        await readyPromise;
        assert.equal(painted, true);
        assert.equal(decoded, true);
        hide();
        show({ ...initial, isAppReady: true, isLight: true, reducedMotion: true,
            branding: { companyLabel: 'LATE BRAND', productLabel: 'NEW PRODUCT' } });
        await Promise.resolve();
        assert.equal(roots, 1);
        assert.equal(frames.length, 2);
        assert.equal(frames[0].type, frames[1].type);
        assert.equal(frames[1].props.isAppReady, true);
        assert.equal(frames[1].props.isLight, false);
        assert.equal(frames[1].props.reducedMotion, false);
        assert.deepEqual(frames[1].props.branding, initial.branding);
        assert.equal(node.hidden, false);
        hide();
        await Promise.resolve();
        assert.equal(node.hidden, true);
        assert.equal(frames.at(-1), null);
    `;
    const process = Bun.spawn([Bun.which('bun')!, '-e', script], {
        stdout: 'pipe', stderr: 'pipe',
    });
    const stderr = await new Response(process.stderr).text();
    expect({ code: await process.exited, stderr }).toEqual({ code: 0, stderr: '' });
});

test('blocked splash styles reject readiness instead of exposing an unstyled window', async () => {
    const script = `
        import { mock } from 'bun:test';
        import { strict as assert } from 'node:assert';
        let link;
        mock.module('react-dom/client', () => ({ createRoot: () => ({ render() {} }) }));
        mock.module('./src/components/StartupAnimation', () => ({ default: () => null }));
        mock.module('./src/components/SplashScreen.css?url', () => ({ default: '/assets/splash.css' }));
        globalThis.document = { createElement: tag => {
            if (tag === 'link') return link = {};
            return { attachShadow: () => ({ append() {} }) };
        }, body: { append() {} } };
        const { showStartupAnimation: show, waitForStartupAnimationReady: ready } =
            await import('./src/startup/animationRoot');
        show({ branding: {}, isLight: false, reducedMotion: false });
        const result = ready();
        link.onerror();
        await assert.rejects(result, /Unable to load startup animation styles/);
    `;
    const process = Bun.spawn([Bun.which('bun')!, '-e', script], {
        stdout: 'pipe', stderr: 'pipe',
    });
    const stderr = await new Response(process.stderr).text();
    expect({ code: await process.exited, stderr }).toEqual({ code: 0, stderr: '' });
});

test('a late stylesheet cannot resurrect a dismissed startup animation', async () => {
    const script = `
        import { mock } from 'bun:test';
        import { strict as assert } from 'node:assert';
        let link;
        const frames = [];
        mock.module('react-dom/client', () => ({ createRoot: () => ({ render: value => frames.push(value) }) }));
        mock.module('./src/components/StartupAnimation', () => ({ default: () => null }));
        mock.module('./src/components/SplashScreen.css?url', () => ({ default: '/assets/splash.css' }));
        globalThis.document = { createElement: tag => {
            if (tag === 'link') return link = {};
            return { attachShadow: () => ({ append() {} }) };
        }, body: { append() {} } };
        const { showStartupAnimation: show, hideStartupAnimation: hide } = await import('./src/startup/animationRoot');
        show({ branding: {}, isLight: false, reducedMotion: false });
        hide();
        await Promise.resolve();
        link.onload();
        await Promise.resolve();
        assert.deepEqual(frames, [null]);
    `;
    const process = Bun.spawn([Bun.which('bun')!, '-e', script], {
        stdout: 'pipe', stderr: 'pipe',
    });
    const stderr = await new Response(process.stderr).text();
    expect({ code: await process.exited, stderr }).toEqual({ code: 0, stderr: '' });
});
