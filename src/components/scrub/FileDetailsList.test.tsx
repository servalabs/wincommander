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
    ],
    ...extra,
  };
}

const batch = (n: number) => Array.from({ length: n }, (_, i) => photo(`IMG_${i}.jpg`));

describe('FileDetailsList', () => {
  test('a few files open fully: every property and its sample value, no nested toggles', () => {
    const html = renderToStaticMarkup(<FileDetailsList results={batch(2)} tintFor={tint} />);
    expect(html.match(/<section[^>]*><button type="button" aria-expanded="true"/g)).toHaveLength(2);
    expect(/<section[^>]*><button type="button" aria-expanded="false"/.test(html)).toBe(false);
    expect(html.match(/<dl[\s\S]*?<\/dl>/g)?.some((list) => list.includes('<button'))).toBe(false);
    expect(html).toContain('Aperture Value');
    expect(html).toContain('Author, account, or organisation identity');
    expect(html).toContain('Create Date:</span><strong');
    expect(html).toContain('2024:05:01 10:00');
    expect(html).toContain('Other examples found in the original:');
    expect(html).toContain('50mm');
    expect(html).toContain('Collapse all');
  });

  test('many files start collapsed with an informative header and no hidden bodies', () => {
    const html = renderToStaticMarkup(<FileDetailsList results={batch(4)} tintFor={tint} />);
    expect(html.match(/aria-expanded="false"/g)).toHaveLength(4);
    expect(html).toContain('IMG_3.jpg');
    expect(html).toContain('data-scrub-kind="images"');
    expect(html).toContain('4 to remove');
    expect(html).toContain('Camera &amp; device 2');
    expect(html).toContain('Dates &amp; times 1');
    expect(html).toContain('Author &amp; edits 1');
    expect(html).not.toContain('Aperture Value');
    expect(html).not.toContain('2024:05:01');
    expect(html).toContain('Expand all');
    expect(html).toContain('File details');
    expect(html).toContain('4 items');
  });

  test('a file that still carries data starts open even in a big batch', () => {
    const results = [...batch(5), photo('leaky.jpg', { dryRun: false, residualFields: [{ category: 'exif', label: 'Serial number', bytes: 1, isIdentifying: true }] })];
    const html = renderToStaticMarkup(<FileDetailsList results={results} tintFor={tint} />);
    expect(html.match(/aria-expanded="true"/g)).toHaveLength(1);
    expect(html).toContain('1 still present');
    expect(html).toContain('Still present:');
  });

  test('hundreds of files render one page of headers with a show-more button', () => {
    const html = renderToStaticMarkup(<FileDetailsList results={batch(313)} tintFor={tint} />);
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

  test('a file with nothing to remove says so while collapsed', () => {
    const clean = photo('clean.jpg', { fieldsStripped: [], sampleValues: [] });
    const html = renderToStaticMarkup(<FileDetailsList results={[...batch(4), clean]} tintFor={tint} />);
    expect(html).toContain('No metadata found — already clean');
  });
});

test('one file header toggles its own details; Expand all and Collapse all toggle every file', async () => {
  const script = String.raw`
    try {
    const { strict: assert } = await import('node:assert');
    const { JSDOM } = await import('jsdom');
    const dom = new JSDOM('<div id="root"></div>', { url: 'http://localhost' });
    globalThis.window = dom.window;
    globalThis.document = dom.window.document;
    globalThis.navigator = dom.window.navigator;
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const { act, createElement: h } = await import('react');
    const { createRoot } = await import('react-dom/client');
    const { default: FileDetailsList } = await import('./src/components/scrub/FileDetailsList');
    const result = (i) => ({
      inputPath: 'C:\\pics\\IMG_' + i + '.jpg', outputPath: '', fileType: 'jpg', bytesIn: 10, bytesOut: 5, dryRun: true,
      fieldsStripped: [{ category: 'exif', label: 'EXIF', bytes: 1, isIdentifying: true, details: ['Aperture Value'] }],
    });
    const host = document.getElementById('root');
    const root = createRoot(host);
    await act(async () => root.render(h(FileDetailsList, { results: [0, 1, 2, 3].map(result), tintFor: () => ({ bg: '', fg: '', border: '' }) })));
    const headers = () => [...host.querySelectorAll('section > button[aria-expanded]')];
    const bulk = () => [...host.querySelectorAll('button')].find((b) => /^(Expand|Collapse) all$/.test(b.textContent));
    const openCount = () => headers().filter((b) => b.getAttribute('aria-expanded') === 'true').length;
    assert.equal(openCount(), 0);
    assert.equal(host.textContent.includes('Aperture Value'), false);
    await act(async () => headers()[1].click());
    assert.equal(headers()[1].getAttribute('aria-expanded'), 'true');
    assert.equal(document.getElementById(headers()[1].getAttribute('aria-controls')).textContent.includes('Aperture Value'), true);
    assert.equal(openCount(), 1);
    await act(async () => bulk().click());
    assert.equal(openCount(), 4);
    assert.equal(bulk().textContent, 'Collapse all');
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
