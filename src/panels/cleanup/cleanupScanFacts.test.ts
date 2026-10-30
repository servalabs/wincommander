import { describe, expect, test } from 'bun:test';
import { getCleanupScanFacts } from './cleanupScanFacts';

describe('cleanup scan details', () => {
  test('keeps returned service state, zero size, and unknown scalar details', () => {
    expect(getCleanupScanFacts({ total: 0, files: [], pcaSvcState: 'Running', totalSizeMB: 0, extraDetail: 'available' })).toEqual([
      { label: 'Compatibility Assistant service', value: 'Running' },
      { label: 'Total size (MB)', value: '0' },
      { label: 'Extra Detail', value: 'available' },
    ]);
  });

  test('keeps both boolean values without treating false as missing', () => {
    expect(getCleanupScanFacts({ clipboardHistoryDisabled: false, cloudClipboardDisabled: true })).toEqual([
      { label: 'Clipboard history disabled', value: 'No' },
      { label: 'Cloud clipboard disabled', value: 'Yes' },
    ]);
  });

  test('retains descriptive coverage while leaving status codes to the coverage indicator', () => {
    expect(getCleanupScanFacts({ coverage: 'Database file metadata only. Historical usage records are not decoded.' })).toEqual([
      { label: 'Coverage', value: 'Database file metadata only. Historical usage records are not decoded.' },
    ]);
    expect(getCleanupScanFacts({ coverage: 'limited' })).toEqual([]);
  });

  test('does not duplicate records or display invalid values', () => {
    expect(getCleanupScanFacts({ records: [{ name: 'Example' }], count: 1, other: null, missing: undefined, invalid: NaN })).toEqual([]);
    expect(getCleanupScanFacts(null)).toEqual([]);
  });

  test('omits raw error messages for failed scans while preserving other returned fields', () => {
    const raw = { error: 'internal error', errorMessage: 'internal detail', message: 'internal message', status: 'failed', checked: 0 };
    expect(getCleanupScanFacts(raw, true)).toEqual([{ label: 'Status', value: 'failed' }, { label: 'Checked', value: '0' }]);
    expect(getCleanupScanFacts({ message: 'available value' })).toEqual([{ label: 'Message', value: 'available value' }]);
  });
});
