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

  test('the list is open by default, with a visible text toggle', () => {
    const html = renderToStaticMarkup(<NotCleanedList count={1} entries={notCleanedEntries({ skippedFiles: [`${ROOT}\\a.xyz`] })} roots={[ROOT]} />);
    expect(html).toContain('aria-expanded="true"');
    expect(html).toContain('a.xyz');
    expect(html).toContain('Hide list');
  });

  test('nothing-to-remove files are listed but not counted as not cleaned', () => {
    const file = (name: string, outcome: ScrubFileOutcome['outcome'], reason: string | null, extra: Partial<ScrubFileOutcome> = {}): ScrubFileOutcome => ({
      path: `${ROOT}\\${name}`, root: ROOT, extension: name.split('.').pop() ?? '', detectedType: null, typeMismatch: false,
      cleanedAs: null, outcome, reason, message: null, outputPath: null, scrubbedIndex: null, removedCategories: [], dryRun: true, ...extra,
    });
    const neutralOnly = notCleanedEntries({ files: [file('a.txt', 'already_clean', 'nothing_to_remove'), file('b.csv', 'already_clean', 'empty_file')] });
    const neutralHtml = renderToStaticMarkup(<NotCleanedList count={2} entries={neutralOnly} roots={[ROOT]} />);
    expect(neutralHtml).toContain('2 files with <strong>nothing to remove</strong>');
    expect(neutralHtml).not.toContain('not cleaned');
    expect(neutralHtml).toContain('a.txt');

    const mixed = notCleanedEntries({
      files: [
        file('a.txt', 'already_clean', 'nothing_to_remove'),
        file('Locked', 'failed', 'access_denied', { detectedType: 'folder', extension: '', message: 'Folder could not be read (os error 5)' }),
      ],
    });
    const mixedHtml = renderToStaticMarkup(<NotCleanedList count={2} entries={mixed} roots={[ROOT]} />);
    expect(mixedHtml).toContain('1 file <strong>not cleaned</strong> — 1 no permission · 1 nothing to remove');
    expect(mixedHtml).toContain('Folder could not be read (os error 5)');
    expect(mixedHtml).toContain('data-kind="folders"');
  });

  test('a long reason gets its own full-width line under the file name, not a narrow column', () => {
    const reason = 'The file is locked by another process and could not be opened for writing (os error 32)';
    const entries = notCleanedEntries({
      files: [{
        path: `${ROOT}\\report.docx`, root: ROOT, extension: 'docx', detectedType: null, typeMismatch: false, cleanedAs: null,
        outcome: 'failed', reason: 'locked', message: reason, outputPath: null, scrubbedIndex: null, removedCategories: [], dryRun: true,
      }],
    });
    const html = renderToStaticMarkup(<NotCleanedList count={1} entries={entries} roots={[ROOT]} />);
    expect(html).toMatch(/<span data-note="true" style="grid-column:2 \/ -1[^"]*">[^<]*os error 32/);
    expect(html).not.toContain('report.docx<span');
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
  test('shows every group with every property at once, with no per-group toggles', () => {
    const groups = groupRemovedProperties([
      { category: 'exif', label: 'Camera & GPS', bytes: 1, isIdentifying: true, details: ['Aperture Value', 'Camera or device model', 'Create Date'] },
    ]);
    const html = renderToStaticMarkup(<RemovedPropertyGroups groups={groups} tintFor={tint} />);
    expect(html).toContain('Camera &amp; device');
    expect(html).toContain('Dates &amp; times');
    expect(html).toContain('Aperture Value');
    expect(html).toContain('Camera or device model');
    expect(html).toContain('Create Date');
    expect(html).not.toContain('aria-expanded');
    expect(html).not.toContain('<button');
  });
});
