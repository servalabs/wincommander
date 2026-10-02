import { expect, test } from 'bun:test';

declare const Bun: {
    which(name: string): string | null;
    spawn(args: string[], options: { stdout: 'pipe'; stderr: 'pipe' }): {
        stderr: ReadableStream<Uint8Array>;
        exited: Promise<number>;
    };
};

test('a background bootstrap keeps content mounted until the dashboard takes ownership', async () => {
    const script = `
        import { mock } from 'bun:test';
        import { strict as assert } from 'node:assert';
        let contentMounted = false;
        let mainLoaded = false;
        let mountedAtDashboardImport = false;
        let visibility;
        mock.module('./src/startup/animationRoot', () => ({
            showStartupAnimation(props) { contentMounted = true; visibility = props.isWindowVisible; },
            hideStartupAnimation() { contentMounted = false; },
        }));
        mock.module('./src/startup/brandingCache', () => ({ readStartupBranding: () => ({}) }));
        mock.module('./src/lib/motionPolicy', () => ({ applyMotionClass() {} }));
        mock.module('react-dom', () => ({ flushSync: callback => callback() }));
        mock.module('./src/hooks/startupWindow', () => ({
            async prepareStartupTheme() {},
            async revealStartupWindow() {
                assert.equal(contentMounted, true, 'native readiness needs existing content');
                return false;
            },
        }));
        mock.module('./src/main', () => {
            mountedAtDashboardImport = contentMounted;
            mainLoaded = true;
            return {};
        });
        globalThis.window = { __TAURI_INTERNALS__: { metadata: { currentWindow: { label: 'main' } } } };
        globalThis.location = { search: '' };
        globalThis.document = { documentElement: { classList: { contains: () => false } } };
        await import('./src/startup');
        for (let attempt = 0; attempt < 100 && !mainLoaded; attempt++) {
            await new Promise(resolve => setTimeout(resolve, 5));
        }
        assert.equal(mainLoaded, true, 'hidden startup must still load the dashboard');
        assert.equal(mountedAtDashboardImport, true, 'tray reveal must never expose an empty document');
        assert.equal(contentMounted, true, 'only the dashboard may dismiss bootstrap content');
        assert.equal(visibility, false, 'background animation remains paused');
    `;
    const process = Bun.spawn([Bun.which('bun')!, '-e', script], {
        stdout: 'pipe', stderr: 'pipe',
    });
    const stderr = await new Response(process.stderr).text();
    expect({ code: await process.exited, stderr }).toEqual({ code: 0, stderr: '' });
});
