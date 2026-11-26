import { describe, expect, test } from 'bun:test';
import {
  classifyRemovedProperty,
  FILE_DETAILS_AUTO_OPEN_MAX,
  formatEngineLine,
  groupRemovedProperties,
  initiallyOpenFiles,
  pairSampleValues,
  parseEngineVersions,
  scrubFileKind,
} from './scrubPresentation';
import type { ScrubResult, StrippedField } from './scrubTypes';

describe('scrubFileKind', () => {
  for (const [path, kind, label] of [
    ['C:\\p\\IMG_1.JPG', 'images', 'JPG'],
    ['C:\\p\\a.heic', 'images', 'HEIC'],
    ['C:\\p\\a.gif', 'images', 'GIF'],
    ['C:\\p\\a.png', 'images', 'PNG'],
    ['C:\\p\\a.CR3', 'raw', 'CR3'],
    ['C:\\p\\logo.svg', 'vector', 'SVG'],
    ['C:\\p\\a.pdf', 'pdf', 'PDF'],
    ['C:\\p\\a.docx', 'word', 'DOCX'],
    ['C:\\p\\a.xlsx', 'excel', 'XLSX'],
    ['C:\\p\\a.pptx', 'slides', 'PPTX'],
    ['C:\\p\\a.mov', 'videos', 'MOV'],
    ['C:\\p\\a.flac', 'audio', 'FLAC'],
    ['C:\\p\\a.zip', 'archives', 'ZIP'],
    ['C:\\p\\a.txt', 'text', 'TXT'],
    ['C:\\p\\a.xyz', 'generic', 'XYZ'],
    ['C:\\p\\README', 'generic', ''],
  ] as [string, string, string][]) {
    test(`${path} -> ${kind} icon labelled ${label}`, () => {
      expect(scrubFileKind(path)).toEqual({ kind: kind as never, label });
    });
  }

  test('engine file type wins over a misleading extension', () => {
    expect(scrubFileKind('C:\\p\\photo.jpg', 'PNG')).toEqual({ kind: 'images', label: 'PNG' });
    expect(scrubFileKind('C:\\p\\scan.jpg', 'pdf')).toEqual({ kind: 'pdf', label: 'PDF' });
  });

  test('a MIME-style file type falls back to the extension label', () => {
    expect(scrubFileKind('C:\\p\\a.jpeg', 'image/jpeg')).toEqual({ kind: 'images', label: 'JPEG' });
  });
});

describe('classifyRemovedProperty', () => {
  for (const [category, detail, group] of [
    ['exif', 'GPS location coordinates', 'gps'],
    ['exif', 'GPS Time Stamp', 'gps'],
    ['exif', 'Camera or device model', 'camera'],
    ['exif', 'Device serial number', 'camera'],
    ['exif', 'Aperture Value', 'camera'],
    ['exif', 'Exposure Time', 'camera'],
    ['exif', 'Date Time Original', 'dates'],
    ['exif', 'Capture or edit date and time', 'dates'],
    ['xmp', 'Author, account, or organisation identity', 'author'],
    ['exif', 'Software', 'author'],
    ['pdfInfo', 'Title', 'document'],
    ['officeCore', 'Custom document properties', 'document'],
    ['thumbnail', 'Embedded preview image', 'thumbnail'],
    ['exif', 'Thumbnail Image', 'thumbnail'],
    ['iccProfile', 'Profile Description', 'color'],
    ['pngText', 'Comments, captions, or description', 'text'],
    ['other', 'JPEG header info', 'other'],
    ['paranoid', 'Randomized file timestamps', 'other'],
  ] as [string, string, string][]) {
    test(`${category} / ${detail} -> ${group}`, () => {
      expect(classifyRemovedProperty(category, detail)).toBe(group as never);
    });
  }

  test('unknown tag names fall back to their category group', () => {
    expect(classifyRemovedProperty('exif', 'Y Cb Cr Positioning')).toBe('camera');
    expect(classifyRemovedProperty('officeApp', 'Zzz')).toBe('document');
    expect(classifyRemovedProperty('somethingNew', 'Zzz')).toBe('other');
  });

  test('a known engine-supplied group overrides the heuristics; an unknown one is ignored', () => {
    expect(classifyRemovedProperty('exif', 'Aperture Value', 'other')).toBe('other');
    expect(classifyRemovedProperty('exif', 'Aperture Value', 'bogus')).toBe('camera');
  });
});

describe('groupRemovedProperties', () => {
  const field = (category: string, details: string[], isIdentifying = true, label = 'L'): StrippedField => ({
    category, label, details, bytes: 10, isIdentifying,
  });

  test('groups in summary order, dedupes repeated names and counts them', () => {
    const groups = groupRemovedProperties([
      field('exif', ['Date Time Original', 'Create Date', 'Aperture Value', 'GPS location coordinates']),
      field('exif', ['Create Date', 'create date']),
      field('iccProfile', ['Profile Description'], false),
    ]);
    expect(groups.map((g) => g.id)).toEqual(['gps', 'camera', 'dates', 'color']);
    const dates = groups.find((g) => g.id === 'dates')!;
    expect(dates.properties.map((p) => [p.detail, p.occurrences])).toEqual([
      ['Create Date', 3],
      ['Date Time Original', 1],
    ]);
    expect(groups.find((g) => g.id === 'color')!.isIdentifying).toBe(false);
  });

  test('a field without details is listed under its label', () => {
    const groups = groupRemovedProperties([field('thumbnail', [], false, 'Embedded document preview')]);
    expect(groups).toHaveLength(1);
    expect(groups[0].properties[0].detail).toBe('Embedded document preview');
  });

  test('blank details are ignored and nothing yields no groups', () => {
    expect(groupRemovedProperties([])).toEqual([]);
    expect(groupRemovedProperties([field('exif', ['  ', ''], true, 'Camera & GPS')])[0].properties[0].detail).toBe('Camera & GPS');
  });
});

describe('engine version line', () => {
  const RAW = 'ExifTool 13.59; qpdf version 12.4.2Run qpdf --copyright to see copyright and license information.';

  test('parses the run-on probe string from v3.6.9', () => {
    expect(parseEngineVersions(RAW)).toEqual({ exiftool: '13.59', qpdf: '12.4.2' });
    expect(formatEngineLine(parseEngineVersions(RAW))).toBe(
      'ExifTool 13.59 · qpdf 12.4.2 · JPEG, PNG, HEIC, PDF, Office, RAW, Photoshop, MP4 and more',
    );
  });

  test('a bare number is the ExifTool version', () => {
    expect(parseEngineVersions('13.59')).toEqual({ exiftool: '13.59' });
    expect(parseEngineVersions('13.59; qpdf 12.4.2')).toEqual({ exiftool: '13.59', qpdf: '12.4.2' });
  });

  test('structured versions win and junk is dropped', () => {
    expect(parseEngineVersions(RAW, { exiftool: 'v13.60', qpdf: '12.5.0' })).toEqual({ exiftool: '13.60', qpdf: '12.5.0' });
    expect(parseEngineVersions(RAW, { exiftool: 'garbage', qpdf: 42 })).toEqual({ exiftool: '13.59', qpdf: '12.4.2' });
  });

  test('unparseable or missing text still yields the formats line, never raw text', () => {
    expect(formatEngineLine(parseEngineVersions('weird output'))).toBe('JPEG, PNG, HEIC, PDF, Office, RAW, Photoshop, MP4 and more');
    expect(formatEngineLine(parseEngineVersions(null))).not.toContain('null');
  });
});

describe('pairSampleValues', () => {
  const groups = groupRemovedProperties([
    { category: 'exif', label: 'EXIF', bytes: 1, isIdentifying: true, details: ['Create Date', 'Camera or device model'] },
  ]);

  test('a sample naming a removed property is shown beside it; the rest stay loose examples', () => {
    const { valueByDetail, unmatched } = pairSampleValues(groups, ['CreateDate: 2024:05:01', 'Lens Model: 50mm', 'no key here']);
    expect([...valueByDetail]).toEqual([['Create Date', '2024:05:01']]);
    expect(unmatched).toEqual([{ key: 'Lens Model', value: '50mm' }, { key: '', value: 'no key here' }]);
  });

  test('empty values are dropped and a second sample for the same property is not lost', () => {
    const { valueByDetail, unmatched } = pairSampleValues(groups, ['Create Date:', 'Create Date: a', 'create_date: b']);
    expect([...valueByDetail]).toEqual([['Create Date', 'a']]);
    expect(unmatched).toEqual([{ key: 'create_date', value: 'b' }]);
  });
});

describe('initiallyOpenFiles', () => {
  const file = (name: string, residual = false): ScrubResult => ({
    inputPath: `C:\\p\\${name}`, outputPath: '', fileType: 'jpg', bytesIn: 1, bytesOut: 1, fieldsStripped: [],
    ...(residual ? { residualFields: [{ category: 'exif', label: 'Serial', bytes: 1, isIdentifying: true }] } : {}),
  });

  test(`up to ${FILE_DETAILS_AUTO_OPEN_MAX} files all start open`, () => {
    const few = Array.from({ length: FILE_DETAILS_AUTO_OPEN_MAX }, (_, i) => file(`${i}.jpg`));
    expect(initiallyOpenFiles(few).size).toBe(FILE_DETAILS_AUTO_OPEN_MAX);
  });

  test('a bigger batch starts collapsed except files that still carry data', () => {
    const many = [...Array.from({ length: FILE_DETAILS_AUTO_OPEN_MAX + 1 }, (_, i) => file(`${i}.jpg`)), file('leak.jpg', true)];
    expect([...initiallyOpenFiles(many)]).toEqual(['C:\\p\\leak.jpg']);
  });
});
