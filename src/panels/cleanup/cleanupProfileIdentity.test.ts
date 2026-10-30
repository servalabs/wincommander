import { describe, expect, test } from 'bun:test';
import { resolveCurrentCleanupProfile } from './cleanupProfileIdentity';

describe('cleanup account identity', () => {
  test('uses a verified current profile, including a renamed account', () => {
    expect(resolveCurrentCleanupProfile([{ name: 'OldFolder', isCurrent: true }, { name: 'Other', isCurrent: false }], 'RenamedAccount')).toBe('OldFolder');
  });
  test('never substitutes a different profile when current identity is missing or inconsistent', () => {
    expect(resolveCurrentCleanupProfile([{ name: 'Other', isCurrent: false }], '')).toBe('');
    expect(resolveCurrentCleanupProfile([{ name: 'Other', isCurrent: false }], 'Other')).toBe('');
    expect(resolveCurrentCleanupProfile([{ name: 'A', isCurrent: true }, { name: 'B', isCurrent: true }], 'A')).toBe('');
  });
  test('supports a matching older response that omits profile flags', () => {
    expect(resolveCurrentCleanupProfile([{ name: 'Folder' }], 'folder')).toBe('Folder');
  });
});
