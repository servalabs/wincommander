import { expect, test } from "bun:test";

declare const Bun: {
    which(name: string): string | null;
    spawn(args: string[], options: { stdout: "pipe"; stderr: "pipe" }): { stderr: ReadableStream<Uint8Array>; exited: Promise<number>; kill(): void };
};

test("file statistics preserves partial coverage, semantic headers, sorting and older-backend fallback", async () => {
    const script = String.raw`
        import { mock } from 'bun:test';
        import { strict as assert } from 'node:assert';
        import { JSDOM } from 'jsdom';
        const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost' });
        globalThis.window = dom.window; globalThis.document = dom.window.document;
        globalThis.navigator = dom.window.navigator; globalThis.IS_REACT_ACT_ENVIRONMENT = true;
        const React = await import('react');
        const { act, createElement: h } = React;
        const { createRoot } = await import('react-dom/client');
        const categories = [{ category: 'images', fileCount: 2, logicalSize: 30, samplePath: null }];
        const extensions = Array.from({ length: 101 }, (_, index) => ({ category: '.ext' + String(index).padStart(3, '0'), fileCount: index + 1, logicalSize: index + 1, samplePath: null }));
        const getDiskFileTypes = async (id) => id === 'old'
            ? { scanId: id, scopePath: 'root', categories, complete: true }
            : id === 'unknown' ? { scanId: id, scopePath: 'root', categories }
            : { scanId: id, scopePath: 'root', categories, extensions, complete: false };
        const backend = { getDiskFileTypes };
        mock.module('@/hooks/useBackend', () => ({ default: () => backend }));
        mock.module('@/hooks/useFileIconData', () => ({ useFileIconData: () => null }));
        mock.module('@/components/ui/bp', () => ({ Icon: () => null, Spinner: () => null }));
        const { default: View } = await import('./src/panels/maintenance/disk-analyzer/DiskFileStatistics');
        const host = document.getElementById('root'); const root = createRoot(host);
        await act(async () => { root.render(h(View, { scanId: 'new', path: 'root' })); });
        assert.match(host.textContent, /Partial scan: these totals cover only files that were scanned/);
        assert.equal(host.querySelector('table caption').textContent, 'Scanned files by category');
        assert.equal(host.querySelectorAll('thead th[scope="col"]').length, 4);
        const button = text => [...host.querySelectorAll('button')].find(item => item.textContent.includes(text));
        await act(async () => { button('Extensions').click(); });
        assert.equal(button('Extensions').getAttribute('aria-pressed'), 'true');
        assert.equal(host.querySelectorAll('tbody tr').length, 100);
        assert.ok(button('Show more'));
        await act(async () => { button('Show more').click(); });
        assert.equal(host.querySelectorAll('tbody tr').length, 101);
        const select = host.querySelector('select');
        await act(async () => { select.value = 'name'; select.dispatchEvent(new dom.window.Event('change', { bubbles: true })); });
        assert.match(host.querySelector('tbody tr th').textContent, /\.ext000/);
        assert.ok(host.querySelector('label[for="' + select.id + '"]'));
        await act(async () => { root.render(h(View, { scanId: 'old', path: 'root' })); });
        assert.match(host.textContent, /Extension details are unavailable/);
        await act(async () => { button('Categories').click(); });
        assert.match(host.textContent, /images/);
        assert.ok(!host.textContent.includes('Partial scan:'));
        await act(async () => { root.render(h(View, { scanId: 'unknown', path: 'root' })); });
        assert.match(host.textContent, /Partial scan: these totals cover only files that were scanned/);
        await act(async () => { root.unmount(); }); dom.window.close();
    `;
    const child = Bun.spawn([Bun.which("bun")!, "-e", script], { stdout: "pipe", stderr: "pipe" });
    const timer = setTimeout(() => child.kill(), 30_000);
    try {
        const stderr = await new Response(child.stderr).text();
        expect({ code: await child.exited, stderr }).toEqual({ code: 0, stderr: "" });
    } finally { clearTimeout(timer); }
}, 45_000);
