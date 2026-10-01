import { expect, test } from 'bun:test';

declare const Bun: {
    which(name: string): string | null;
    spawn(args: string[], options: { stdout: 'pipe'; stderr: 'pipe' }): {
        stderr: ReadableStream<Uint8Array>;
        exited: Promise<number>;
    };
};

test('actual polling hooks respect startup, visibility, resume, and in-flight ownership', async () => {
    // Isolate React/context mocks from the rest of the frontend suite.
    const script = `
        import { mock } from 'bun:test';
        import { strict as assert } from 'node:assert';
        const actualReact = await import('react');

        const slots = [];
        let cursor = 0;
        let pendingEffects = [];
        let mounted = true;
        let render;
        const equalDeps = (a, b) => a && b && a.length === b.length && a.every((v, i) => Object.is(v, b[i]));
        function useEffect(effect, deps) {
            const index = cursor++;
            const previous = slots[index];
            if (equalDeps(previous?.deps, deps)) return;
            const next = { deps, cleanup: previous?.cleanup };
            slots[index] = next;
            pendingEffects.push(() => {
                next.cleanup?.();
                next.cleanup = effect();
            });
        }
        mock.module('react', () => ({
            ...actualReact,
            useEffect,
            useRef: initial => {
                const index = cursor++;
                return slots[index] ??= { current: initial };
            },
            useCallback: (callback, deps) => {
                const index = cursor++;
                if (!equalDeps(slots[index]?.deps, deps)) slots[index] = { deps, callback };
                return slots[index].callback;
            },
            useSyncExternalStore: (subscribe, getSnapshot) => {
                useEffect(() => subscribe(() => { if (mounted) render(); }), [subscribe]);
                return getSnapshot();
            },
        }));

        let now = 0;
        let nextTimer = 1;
        const timers = new Map();
        const addTimer = (callback, delay, repeat) => {
            const id = nextTimer++;
            timers.set(id, { callback, delay, due: now + delay, repeat });
            return id;
        };
        globalThis.setInterval = (callback, delay) => addTimer(callback, delay, true);
        globalThis.setTimeout = (callback, delay) => addTimer(callback, delay, false);
        globalThis.clearInterval = globalThis.clearTimeout = id => timers.delete(id);
        const settle = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };
        const advance = async milliseconds => {
            const end = now + milliseconds;
            while (true) {
                const next = [...timers].filter(([, timer]) => timer.due <= end)
                    .sort((a, b) => a[1].due - b[1].due)[0];
                if (!next) break;
                const [id, timer] = next;
                now = timer.due;
                if (timer.repeat) timer.due += timer.delay;
                else timers.delete(id);
                timer.callback();
                await settle();
            }
            now = end;
        };

        const visibilityListeners = new Set();
        globalThis.document = {
            visibilityState: 'visible',
            addEventListener: (type, callback) => {
                assert.equal(type, 'visibilitychange');
                visibilityListeners.add(callback);
            },
            removeEventListener: (type, callback) => visibilityListeners.delete(callback),
        };
        const visibility = async value => {
            document.visibilityState = value;
            for (const callback of [...visibilityListeners]) callback();
            await settle();
        };

        const calls = { metrics: 0, dashboard: 0, drive: 0, network: 0, privacy: 0, vault: 0 };
        let activeRequests = 0;
        let peakRequests = 0;
        let releaseNetwork;
        let delayNetwork = false;
        const state = {
            startupComplete: false,
            appSettings: { app: { modules: { network: true, privacy: true, vault: true } } },
            refreshDashboard: async () => { calls.dashboard++; },
            refreshDriveHealth: async () => { calls.drive++; },
            refreshNetwork: async () => {
                calls.network++;
                peakRequests = Math.max(peakRequests, ++activeRequests);
                try {
                    if (delayNetwork) await new Promise(resolve => { releaseNetwork = resolve; });
                } finally { activeRequests--; }
            },
            refreshPrivacy: async () => {
                calls.privacy++;
                peakRequests = Math.max(peakRequests, ++activeRequests);
                await Promise.resolve();
                activeRequests--;
            },
            refreshHardening: async () => {}, refreshMesh: async () => {},
            refreshProductivity: async () => {},
            refreshVault: async () => { calls.vault++; },
        };
        const live = { refreshLiveMetrics: async () => { calls.metrics++; } };
        mock.module('./src/types/panels', () => ({ PANEL_MANIFESTS: [
            { id: 'dashboard', refreshKey: 'refreshDashboard' },
            { id: 'network', refreshKey: 'refreshNetwork' },
            { id: 'privacy', refreshKey: 'refreshPrivacy' },
            { id: 'vault', refreshKey: 'refreshVault' },
        ] }));
        mock.module('./src/context/AppContext', () => ({ useAppState: () => state }));
        mock.module('./src/context/LiveMetricsContext', () => ({ useLiveMetrics: () => live }));
        const { useActivePanelPoller } = await import('./src/hooks/useActivePanelPoller');
        let activePanel = 'dashboard';
        let paused = false;
        render = () => {
            cursor = 0;
            pendingEffects = [];
            useActivePanelPoller({ activePanel, paused });
            const effects = pendingEffects;
            pendingEffects = [];
            effects.forEach(effect => effect());
        };

        render();
        await advance(60_000);
        assert.deepEqual(calls, { metrics: 0, dashboard: 0, drive: 0, network: 0, privacy: 0, vault: 0 }, 'no polling before startup');
        assert.equal(timers.size, 0);
        state.startupComplete = true;
        render();
        await settle();
        assert.equal(calls.metrics, 1, 'startup causes one immediate metrics refresh');
        await advance(2_000);
        assert.equal(calls.metrics, 2);

        await visibility('hidden');
        const hiddenCalls = { ...calls };
        assert.equal(timers.size, 0, 'hiding cancels polls and deferred SMART work');
        await advance(60_000);
        assert.deepEqual(calls, hiddenCalls);
        await visibility('visible');
        assert.equal(calls.metrics, hiddenCalls.metrics + 1, 'showing resumes immediately once');
        await visibility('visible');
        assert.equal(calls.metrics, hiddenCalls.metrics + 1, 'duplicate visible event adds no refresh');

        paused = true;
        render();
        await advance(10_000);
        assert.equal(calls.metrics, hiddenCalls.metrics + 1, 'explicit pause remains authoritative');
        paused = false;
        activePanel = 'network';
        delayNetwork = true;
        render();
        await settle();
        assert.equal(calls.network, 1);
        await advance(30_000);
        assert.equal(calls.network, 1, 'slow request owns the refresh slot across ticks');
        activePanel = 'privacy';
        render();
        await visibility('hidden');
        await visibility('visible');
        activePanel = 'network';
        render();
        activePanel = 'privacy';
        render();
        await advance(10_000);
        assert.equal(calls.privacy, 0, 'rapid panel and visibility changes cannot overlap the old request');
        releaseNetwork();
        await settle();
        delayNetwork = false;
        await advance(10_000);
        assert.equal(calls.privacy, 1, 'active panel resumes after prior work settles');
        assert.equal(peakRequests, 1);
        await visibility('hidden');
        const beforeResume = calls.privacy;
        await advance(30_000);
        assert.equal(calls.privacy, beforeResume);
        await visibility('visible');
        assert.equal(calls.privacy, beforeResume + 1, 'panel visibility resume refreshes once immediately');

        state.encryptionStatus = {};
        activePanel = 'vault';
        render();
        await settle();
        assert.equal(calls.vault, 0, 'existing Vault observations retain their no-extra-entry-read contract');
        await advance(5_000);
        assert.equal(calls.vault, 1);
        await visibility('hidden');
        await advance(20_000);
        assert.equal(calls.vault, 1);
        await visibility('visible');
        assert.equal(calls.vault, 1, 'Vault resume does not add an extra observed-state read');
        await advance(5_000);
        assert.equal(calls.vault, 2);

        mounted = false;
        slots.forEach(slot => slot?.cleanup?.());
        assert.equal(visibilityListeners.size, 0, 'unmount removes the real visibility subscription');
        assert.equal(timers.size, 0, 'unmount cancels scheduled refreshes');
    `;
    const process = Bun.spawn([Bun.which('bun')!, '-e', script], {
        stdout: 'pipe', stderr: 'pipe',
    });
    const stderr = await new Response(process.stderr).text();
    expect({ code: await process.exited, stderr }).toEqual({ code: 0, stderr: '' });
});
