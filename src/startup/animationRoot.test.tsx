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
        const node = { hidden: false, attachShadow: () => ({ append() {} }) };
        mock.module('react-dom/client', () => ({ createRoot: () => {
            roots++;
            return { render: value => frames.push(value) };
        } }));
        mock.module('./src/components/StartupAnimation', () => ({ default: () => null }));
        mock.module('./src/components/SplashScreen.css?inline', () => ({ default: '' }));
        globalThis.document = { createElement: () => node, body: { append() {} } };
        const { showStartupAnimation: show, hideStartupAnimation: hide } = await import('./src/startup/animationRoot');
        const initial = { branding: { companyLabel: 'SERVAlABS', productLabel: 'WINCOMMANDER' },
            isLight: false, reducedMotion: false, isAppReady: false,
            startupError: null, onComplete() {}, onRetry() {} };
        show(initial);
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
