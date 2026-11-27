import { describe, expect, test } from 'bun:test';
import { renderToStaticMarkup } from 'react-dom/server';
import FileDetailsList, { FILE_DETAILS_PAGE_SIZE } from './FileDetailsList';
import type { ScrubResult } from '@/lib/scrubTypes';

declare const Bun: {
  which(name: string): string | null;
  spawn(args: string[], options: { stdout: 'pipe'; stderr: 'pipe' }): {
    stderr: ReadableStream<Uint8Array>; exited: Promise<number>; kill(): void;
  };
};

const tint = () => ({ bg: 'b', fg: 'f', border: 'o' });

function photo(name: string, extra: Partial<ScrubResult> = {}): ScrubResult {
  return {
    inputPath: `C:\\pics\\${name}`, outputPath: '', fileType: 'jpg', bytesIn: 2048, bytesOut: 1024, dryRun: true,
    sampleValues: ['CreateDate: 2024:05:01 10:00', 'Lens: 50mm'],
    fieldsStripped: [
      { category: 'exif', label: 'EXIF', bytes: 1, isIdentifying: true, details: ['Camera or device model', 'Create Date', 'Aperture Value'] },
      { category: 'xmp', label: 'XMP', bytes: 1, isIdentifying: true, details: ['Author, account, or organisation identity'] },
      { category: 'iccProfile', label: 'ICC', bytes: 1, isIdentifying: false, details: ['Profile Description'] },
    ],
    ...extra,
  };
}

const batch = (n: number) => Array.from({ length: n }, (_, i) => photo(`IMG_${i}.jpg`));
// The (i) info buttons are popover triggers that carry aria-expanded too.
const dropdowns = (html: string, state: 'true' | 'false') => html.match(new RegExp(`<button type="button" aria-expanded="${state}"`, 'g')) ?? [];
const GRID = /display:grid;grid-template-columns:repeat\(auto-fit, minmax\(min\(300px, 100%\), 1fr\)\)[^"]*align-items:start/;

describe('FileDetailsList', () => {
  test('files sit side by side as cards in a top-aligned grid', () => {
    const html = renderToStaticMarkup(<FileDetailsList results={batch(4)} tintFor={tint} />);
    expect(html).toMatch(GRID);
    expect(html.match(/<section aria-label="IMG_\d\.jpg"/g)).toHaveLength(4);
    expect(html).toContain('data-scrub-kind="images"');
  });

  test('a few files open fully: every kind and every property at once, no nested toggles', () => {
    const html = renderToStaticMarkup(<FileDetailsList results={batch(2)} tintFor={tint} />);
    expect(dropdowns(html, 'true')).toHaveLength(2);
    expect(dropdowns(html, 'false')).toHaveLength(0);
    expect(html.match(/<dl[\s\S]*?<\/dl>/g)?.some((list) => list.includes('<button'))).toBe(false);
    expect(html).toContain('Will remove from this file:');
    expect(html).toContain('Aperture Value');
    expect(html).toContain('Author, account, or organisation identity');
    expect(html).toContain('Profile Description');
    expect(html).toContain('Collapse all');
  });

  test('many files start collapsed: examples and the per-kind summary show, removed-property chips do not render', () => {
    const html = renderToStaticMarkup(<FileDetailsList results={batch(4)} tintFor={tint} />);
    expect(dropdowns(html, 'false')).toHaveLength(4);
    expect(html).toContain('Examples found in the original:');
    expect(html).toContain('2024:05:01 10:00');
    expect(html).toContain('50mm');
    expect(html).toContain('<strong>5 items</strong>');
    expect(html).toContain('<span style="color:var(--color-danger)">Camera &amp; device 2</span>');
    expect(html).toContain('<span style="color:var(--color-danger)">Author &amp; edits 1</span>');
    expect(html).toContain('<span>Color profile 1</span>');
    expect(html).not.toContain('<dl');
    expect(html).not.toContain('Aperture Value');
    expect(html).not.toContain('Profile Description');
    expect(html).toContain('Expand all');
    expect(html).toContain('File details');
    expect(html).toContain('4 items');
  });

  test('the GPS location stays visible on a collapsed card', () => {
    const gps = photo('trip.jpg', { gpsCoords: { lat: 1, lon: 2, label: '1.0000, 2.0000' } as ScrubResult['gpsCoords'] });
    const html = renderToStaticMarkup(<FileDetailsList results={[...batch(4), gps]} tintFor={tint} />);
    expect(dropdowns(html, 'true')).toHaveLength(0);
    expect(html).toContain('Location to remove:');
    expect(html).toContain('1.0000, 2.0000');
  });

  test('a file that still carries data starts open even in a big batch', () => {
    const results = [...batch(5), photo('leaky.jpg', { dryRun: false, residualFields: [{ category: 'exif', label: 'Serial number', bytes: 1, isIdentifying: true }] })];
    const html = renderToStaticMarkup(<FileDetailsList results={results} tintFor={tint} />);
    expect(dropdowns(html, 'true')).toHaveLength(1);
    expect(html).toContain('Removed from this file:');
    expect(html).toContain('Still present:');
    expect(html).toContain('Serial number');
  });

  test('hundreds of files render one page of cards with a show-more button', () => {
    const html = renderToStaticMarkup(<FileDetailsList results={batch(313)} tintFor={tint} />);
    expect(html.match(/<section /g)).toHaveLength(FILE_DETAILS_PAGE_SIZE);
    expect(html).toContain(`IMG_${FILE_DETAILS_PAGE_SIZE - 1}.jpg`);
    expect(html).not.toContain(`IMG_${FILE_DETAILS_PAGE_SIZE}.jpg`);
    expect(html).toContain(`Show ${FILE_DETAILS_PAGE_SIZE} more (${313 - FILE_DETAILS_PAGE_SIZE} not shown)`);
    expect(html).toContain('313 items');
  });

  test('inside a folder section the title is dropped but the bulk toggle stays', () => {
    const html = renderToStaticMarkup(<FileDetailsList results={batch(4)} tintFor={tint} showTitle={false} />);
    expect(html).not.toContain('File details');
    expect(html).toContain('Expand all');
    const single = renderToStaticMarkup(<FileDetailsList results={batch(1)} tintFor={tint} showTitle={false} />);
    expect(single).not.toContain('Expand all');
    expect(single).not.toContain('Collapse all');
  });

  test('a file with nothing to remove says so and offers no empty dropdown', () => {
    const clean = photo('clean.jpg', { fieldsStripped: [], sampleValues: [] });
    const html = renderToStaticMarkup(<FileDetailsList results={[clean]} tintFor={tint} />);
    expect(html).toContain('No metadata found — already clean');
    expect(html).not.toContain('aria-expanded');
  });
});

test('a card dropdown toggles its whole removal section; Expand all and Collapse all toggle every card; Show more pages', async () => {
  const script = String.raw`
    try {
    const { strict: assert } = await import('node:assert');
    const { JSDOM } = await import('jsdom');
    const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost' });
    globalThis.window = dom.window;
    globalThis.document = dom.window.document;
    globalThis.navigator = dom.window.navigator;
    globalThis.getComputedStyle = dom.window.getComputedStyle;
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const { act, createElement: h } = await import('react');
    const { createRoot } = await import('react-dom/client');
    const { default: FileDetailsList, FILE_DETAILS_PAGE_SIZE } = await import('./src/components/scrub/FileDetailsList');
    const result = (i) => ({
      inputPath: 'C:\\pics\\IMG_' + i + '.jpg', outputPath: '', fileType: 'jpg', bytesIn: 10, bytesOut: 5, dryRun: true,
      sampleValues: ['Lens: 50mm'],
      fieldsStripped: [{ category: 'exif', label: 'EXIF', bytes: 1, isIdentifying: true, details: ['Aperture Value', 'Create Date'] }],
    });
    const total = FILE_DETAILS_PAGE_SIZE + 4;
    const host = document.getElementById('root');
    const root = createRoot(host);
    await act(async () => root.render(h(FileDetailsList, { results: Array.from({ length: total }, (_, i) => result(i)), tintFor: () => ({ bg: '', fg: '', border: '' }) })));
    const toggles = () => [...host.querySelectorAll('section button[aria-expanded]:not([aria-haspopup])')];
    const bulk = () => [...host.querySelectorAll('button')].find((b) => /^(Expand|Collapse) all$/.test(b.textContent));
    const more = () => [...host.querySelectorAll('button')].find((b) => b.textContent.startsWith('Show '));
    const openCount = () => toggles().filter((b) => b.getAttribute('aria-expanded') === 'true').length;
    const cards = () => host.querySelectorAll('section');
    assert.equal(cards().length, FILE_DETAILS_PAGE_SIZE);
    assert.equal(openCount(), 0);
    assert.equal(host.textContent.includes('Aperture Value'), false);
    assert.equal(cards()[1].textContent.includes('50mm'), true);
    assert.equal(toggles()[1].tagName, 'BUTTON');
    await act(async () => toggles()[1].click());
    assert.equal(toggles()[1].getAttribute('aria-expanded'), 'true');
    const body = document.getElementById(toggles()[1].getAttribute('aria-controls'));
    assert.equal(body.textContent.includes('Aperture Value'), true);
    assert.equal(body.textContent.includes('Create Date'), true);
    assert.equal(body.querySelectorAll('button').length, 0);
    assert.equal(openCount(), 1);
    await act(async () => toggles()[1].click());
    assert.equal(openCount(), 0);
    assert.equal(host.textContent.includes('Aperture Value'), false);
    await act(async () => bulk().click());
    assert.equal(openCount(), FILE_DETAILS_PAGE_SIZE);
    assert.equal(bulk().textContent, 'Collapse all');
    await act(async () => more().click());
    assert.equal(cards().length, total);
    assert.equal(more(), undefined);
    assert.equal(openCount(), total);
    await act(async () => bulk().click());
    assert.equal(openCount(), 0);
    assert.equal(bulk().textContent, 'Expand all');
    await act(async () => root.unmount());
    dom.window.close();
    process.exit(0);
    } catch (error) { console.error(error); process.exit(1); }
  `;
  const child = Bun.spawn([Bun.which('bun')!, '-e', script], { stdout: 'pipe', stderr: 'pipe' });
  const timer = setTimeout(() => child.kill(), 90_000);
  try {
    const stderr = await new Response(child.stderr).text();
    expect({ code: await child.exited, stderr }).toEqual({ code: 0, stderr: '' });
  } finally { clearTimeout(timer); }
}, 105_000);
