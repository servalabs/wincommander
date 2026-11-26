import { describe, expect, test } from 'bun:test';
import {
  aggregateByRoot,
  displayFolder,
  folderStatusText,
  groupNotCleaned,
  notCleanedEntries,
  owningRoot,
  shouldShowFolderSections,
  skipReasonId,
  skipReasonLabel,
} from './scrubReportModel';
import type { ScrubFileOutcome, ScrubResult, ScrubRootSummary } from './scrubTypes';

const DOCS = 'C:\\Users\\u\\Documents';
const PICS = 'C:\\Users\\u\\Pictures';

const result = (inputPath: string, data: boolean, residual = false): ScrubResult => ({
  inputPath,
  outputPath: `${inputPath}.out`,
  fileType: 'JPG',
  bytesIn: 10,
  bytesOut: 5,
  fieldsStripped: data ? [{ category: 'exif', label: 'Camera', bytes: 5, isIdentifying: true }] : [],
  ...(residual ? { residualFields: [{ category: 'exif', label: 'Left', bytes: 0, isIdentifying: true }] } : {}),
});

const outcome = (path: string, root: string, o: Partial<ScrubFileOutcome>): ScrubFileOutcome => ({
  path, root, extension: path.split('.').pop() ?? '', detectedType: null, typeMismatch: false, cleanedAs: null,
  outcome: 'cleaned', reason: null, message: null, outputPath: null, scrubbedIndex: null, removedCategories: [], dryRun: true,
  ...o,
});

const summary = (root: string, isDirectory: boolean, c: Partial<ScrubRootSummary>): ScrubRootSummary => ({
  root, isDirectory, exists: true, total: 0, cleaned: 0, alreadyClean: 0, residual: 0, notCleaned: 0, failed: 0, ...c,
});

describe('skip reasons', () => {
  for (const [code, id, label] of [
    ['unsupported_format', 'unsupported', 'Unsupported format'],
    ['content_mismatch', 'mismatch', 'Type mismatch'],
    ['locked', 'locked', 'In use by another app'],
    ['access_denied', 'accessDenied', 'No permission'],
    ['read_failed', 'readFailed', "Couldn't read"],
    ['write_failed', 'writeFailed', "Couldn't write"],
    ['too_long_path', 'pathTooLong', 'Path too long'],
    ['empty_file', 'emptyFile', 'Empty file — nothing to remove'],
    ['nothing_to_remove', 'nothingToRemove', 'Nothing to remove'],
    ['contentMismatch', 'mismatch', 'Type mismatch'],
    ['', 'unreported', 'Unsupported format or content mismatch'],
    [null, 'unreported', 'Unsupported format or content mismatch'],
    ['quantum_flux', 'other', 'Other reason'],
  ] as [string | null, string, string][]) {
    test(`${String(code)} -> ${id}`, () => {
      expect(skipReasonId(code)).toBe(id as never);
      expect(skipReasonLabel(skipReasonId(code))).toBe(label);
    });
  }

  test('older engines: bare skipped paths are listed with the combined reason; junk is dropped', () => {
    expect(notCleanedEntries({ skippedFiles: ['C:\\a\\x.abc', '', 42 as never] })).toEqual([
      { path: 'C:\\a\\x.abc', reason: 'unreported' },
    ]);
    expect(notCleanedEntries({})).toEqual([]);
  });

  test('per-file outcomes: lists files without a card, with the specific reason and note', () => {
    const entries = notCleanedEntries({
      skippedFiles: ['ignored when files[] is present'],
      files: [
        outcome(`${PICS}\\a.jpg`, PICS, { outcome: 'cleaned', scrubbedIndex: 0 }),
        outcome(`${PICS}\\b.jpg`, PICS, { outcome: 'not_cleaned', reason: 'content_mismatch', detectedType: 'png', typeMismatch: true }),
        outcome(`${PICS}\\c.docx`, PICS, { outcome: 'failed', reason: 'locked', message: 'The process cannot access the file' }),
        outcome(`${PICS}\\d.txt`, PICS, { outcome: 'already_clean', reason: 'nothing_to_remove' }),
        outcome(`${PICS}\\e.jpg`, PICS, { outcome: 'already_clean', reason: 'nothing_to_remove', scrubbedIndex: 1 }),
        outcome(`${PICS}\\f.jpg`, PICS, { outcome: 'not_cleaned', reason: 'content_mismatch', cleanedAs: 'png' }),
        outcome(`${PICS}\\g.bin`, PICS, { outcome: 'failed' }),
        outcome(`${PICS}\\h.zz`, PICS, { outcome: 'residual', scrubbedIndex: 2 }),
      ],
    });
    expect(entries).toEqual([
      { path: `${PICS}\\b.jpg`, reason: 'mismatch', note: 'named .jpg but is really PNG' },
      { path: `${PICS}\\c.docx`, reason: 'locked', note: 'The process cannot access the file' },
      { path: `${PICS}\\d.txt`, reason: 'nothingToRemove' },
      { path: `${PICS}\\f.jpg`, reason: 'mismatch', note: 'cleaned as PNG' },
      { path: `${PICS}\\g.bin`, reason: 'failed' },
    ]);
  });

  test('groups come out problems first, neutral reasons last', () => {
    const groups = groupNotCleaned([
      { path: 'a', reason: 'nothingToRemove' },
      { path: 'b', reason: 'unsupported' },
      { path: 'c', reason: 'locked' },
      { path: 'd', reason: 'unsupported' },
    ]);
    expect(groups.map((g) => [g.label, g.entries.length])).toEqual([
      ['In use by another app', 1],
      ['Unsupported format', 2],
      ['Nothing to remove', 1],
    ]);
  });
});

describe('per-folder aggregation from the engine summary', () => {
  test('uses roots[] counts and files[].root exactly, keeping the order sent', () => {
    const scrubbed = [result(`${DOCS}\\a.pdf`, true), result(`${PICS}\\b.jpg`, true, true)];
    const buckets = aggregateByRoot(
      {
        scrubbed,
        files: [
          outcome(`${DOCS}\\a.pdf`, DOCS, { scrubbedIndex: 0 }),
          outcome(`${PICS}\\b.jpg`, PICS, { outcome: 'residual', scrubbedIndex: 1 }),
          outcome(`${PICS}\\c.xyz`, PICS, { outcome: 'not_cleaned', reason: 'unsupported_format' }),
          outcome('D:\\loose.jpg', 'D:\\loose.jpg', { scrubbedIndex: 7 }),
        ],
        roots: [
          summary(PICS, true, { total: 2, residual: 1, notCleaned: 1 }),
          summary(DOCS, true, { total: 36, cleaned: 30, alreadyClean: 6 }),
          summary('D:\\loose.jpg', false, { total: 1, failed: 1 }),
        ],
      },
      [],
      [],
      new Set(),
    );
    expect(buckets.map((b) => [b.name, b.isFolder, b.results.length, b.total, b.toClean, b.nothingToRemove, b.notCleaned, b.stillPresent])).toEqual([
      ['Pictures', true, 1, 2, 1, 0, 1, 1],
      ['Documents', true, 1, 36, 30, 6, 0, 0],
      ['Individual files', false, 0, 1, 0, 0, 1, 0],
    ]);
    expect(folderStatusText(buckets[1], true)).toBe('Ready — 36 files, 30 to clean, 6 nothing to remove');
    expect(folderStatusText(buckets[0], false)).toBe('Done — 2 files, 0 cleaned, 0 nothing to remove, 1 not cleaned, 1 still contain data');
    expect(shouldShowFolderSections(buckets)).toBe(true);
  });
});

describe('per-folder aggregation from paths (older engines)', () => {
  test('splits by queued folder, case- and slash-insensitively', () => {
    const buckets = aggregateByRoot(
      { scrubbed: [result(`${DOCS}\\a.pdf`, true), result('c:/users/u/documents/sub/b.docx', false), result(`${PICS}\\c.jpg`, true, true)] },
      [{ path: `${PICS}\\raw\\d.xyz`, reason: 'unreported' }],
      [DOCS, PICS],
      new Set(),
    );
    expect(buckets.map((b) => [b.name, b.isFolder, b.results.length, b.total, b.toClean, b.cleaned, b.nothingToRemove, b.notCleaned, b.stillPresent])).toEqual([
      ['Documents', true, 2, 2, 1, 1, 1, 0, 0],
      ['Pictures', true, 1, 2, 1, 0, 0, 1, 1],
    ]);
  });

  test('nested queued folders own their own files; a sibling prefix is not a child', () => {
    expect(owningRoot(`${PICS}\\2024\\x.jpg`, [PICS, `${PICS}\\2024`])).toBe(`${PICS}\\2024`);
    expect(owningRoot(`${PICS}X\\x.jpg`, [PICS])).toBeUndefined();
  });

  test('loose queued files share one bucket; one folder or only files shows no sections', () => {
    const buckets = aggregateByRoot(
      { scrubbed: [result(`${DOCS}\\a.pdf`, true), result('D:\\one.jpg', true), result('D:\\two.jpg', false)] },
      [],
      [DOCS, 'D:\\one.jpg', 'D:\\two.jpg'],
      new Set([DOCS]),
    );
    expect(buckets.map((b) => [b.name, b.isFolder, b.results.length])).toEqual([
      ['Documents', true, 1],
      ['Individual files', false, 2],
    ]);
    expect(shouldShowFolderSections(buckets)).toBe(true);
    expect(shouldShowFolderSections(aggregateByRoot({ scrubbed: [result(`${DOCS}\\a.pdf`, true)] }, [], [DOCS], new Set()))).toBe(false);
    expect(shouldShowFolderSections(aggregateByRoot({ scrubbed: [result('D:\\one.jpg', true), result('D:\\two.jpg', true)] }, [], ['D:\\one.jpg', 'D:\\two.jpg'], new Set()))).toBe(false);
  });

  test('a picked folder with no supported files still gets its own section', () => {
    const buckets = aggregateByRoot({ scrubbed: [result(`${DOCS}\\a.pdf`, true)] }, [], [DOCS, PICS], new Set([PICS]));
    expect(buckets.map((b) => [b.name, b.results.length])).toEqual([['Documents', 1], ['Pictures', 0]]);
  });

  test('status line wording for preview and real runs', () => {
    const scrubbed = [...Array(30)].map((_, i) => result(`${DOCS}\\f${i}.jpg`, true))
      .concat([...Array(6)].map((_, i) => result(`${DOCS}\\n${i}.jpg`, false)));
    const [docs] = aggregateByRoot({ scrubbed }, [], [DOCS], new Set([DOCS]));
    expect(folderStatusText(docs, true)).toBe('Ready — 36 files, 30 to clean, 6 nothing to remove');
    expect(folderStatusText(docs, false)).toBe('Done — 36 files, 30 cleaned, 6 nothing to remove');
  });
});

test('display folder is relative to the queued folder', () => {
  expect(displayFolder(`${PICS}\\2024\\Trip\\a.jpg`, [PICS])).toBe('Pictures\\2024\\Trip');
  expect(displayFolder(`${PICS}\\a.jpg`, [PICS])).toBe('Pictures');
  expect(displayFolder('D:\\x\\one.jpg', ['D:\\x\\one.jpg'])).toBe('D:\\x');
});
