import { describe, expect, test } from 'bun:test';
import { renderToStaticMarkup } from 'react-dom/server';
import NotCleanedList, { NOT_CLEANED_PAGE_SIZE } from './NotCleanedList';
import RemovedPropertyGroups from './RemovedPropertyGroups';
import { notCleanedEntries } from '@/lib/scrubReportModel';
import { groupRemovedProperties } from '@/lib/scrubPresentation';
import type { ScrubFileOutcome } from '@/lib/scrubTypes';

const ROOT = 'C:\\Users\\u\\Documents';
const tint = () => ({ bg: 'b', fg: 'f', border: 'o' });

describe('NotCleanedList', () => {
  test('lists every bare-path file instead of an empty box (v3.6.9 regression)', () => {
    const paths = Array.from({ length: 208 }, (_, i) => `${ROOT}\\sub\\file${i}.abc`);
    const html = renderToStaticMarkup(
      <NotCleanedList count={208} entries={notCleanedEntries({ skippedFiles: paths })} roots={[ROOT]} defaultExpanded />,
    );
    expect(html).toContain('208 files <strong>not cleaned</strong>');
    expect(html).toContain('Unsupported format or content mismatch');
    expect(html).toContain('file0.abc');
    expect(html).toContain(`file${NOT_CLEANED_PAGE_SIZE - 1}.abc`);
    expect(html).not.toContain(`file${NOT_CLEANED_PAGE_SIZE}.abc`);
    expect(html).toContain(`Show ${NOT_CLEANED_PAGE_SIZE} more (${208 - NOT_CLEANED_PAGE_SIZE} not shown)`);
    expect(html).toContain('Documents\\sub');
    expect(html).not.toContain('overflow-y:auto');
  });

  test('groups by engine reason and shows the per-file detail', () => {
    const file = (name: string, outcome: ScrubFileOutcome['outcome'], reason: string, extra: Partial<ScrubFileOutcome> = {}): ScrubFileOutcome => ({
      path: `${ROOT}\\${name}`, root: ROOT, extension: name.split('.').pop() ?? '', detectedType: null, typeMismatch: false,
      cleanedAs: null, outcome, reason, message: null, outputPath: null, scrubbedIndex: null, removedCategories: [], dryRun: true, ...extra,
    });
    const entries = notCleanedEntries({
      files: [
        file('a.jpg', 'not_cleaned', 'content_mismatch', { detectedType: 'png', typeMismatch: true }),
        file('b.docx', 'failed', 'locked'),
        file('c.xyz', 'not_cleaned', 'unsupported_format'),
      ],
    });
    const html = renderToStaticMarkup(<NotCleanedList count={3} entries={entries} roots={[ROOT]} defaultExpanded />);
    expect(html).toContain('1 in use by another app · 1 type mismatch · 1 unsupported format');
    expect(html.indexOf('Unsupported format')).toBeGreaterThan(html.indexOf('In use by another app'));
    expect(html).toContain('named .jpg but is really PNG');
    expect(html).toContain('data-scrub-kind="word"');
  });

  test('count without names is reported and not expandable', () => {
    const html = renderToStaticMarkup(<NotCleanedList count={5} entries={[]} roots={[]} defaultExpanded />);
    expect(html).toContain('5 files <strong>not cleaned</strong> — unsupported format or content mismatch');
    expect(html).not.toContain('aria-expanded');
  });

  test('names missing from a partial list are counted', () => {
    const html = renderToStaticMarkup(
      <NotCleanedList count={4} entries={notCleanedEntries({ skippedFiles: [`${ROOT}\\a.xyz`] })} roots={[ROOT]} defaultExpanded />,
    );
    expect(html).toContain('3 more files were not cleaned but not named by the engine.');
  });
});

describe('RemovedPropertyGroups', () => {
  test('starts collapsed to group names with counts', () => {
    const groups = groupRemovedProperties([
      { category: 'exif', label: 'Camera & GPS', bytes: 1, isIdentifying: true, details: ['Aperture Value', 'Camera or device model', 'Create Date'] },
    ]);
    const html = renderToStaticMarkup(<RemovedPropertyGroups groups={groups} tintFor={tint} />);
    expect(html).toContain('Camera &amp; device');
    expect(html).toContain('Dates &amp; times');
    expect(html).toContain('aria-expanded="false"');
    expect(html).not.toContain('Aperture Value');
  });
});
