import { describe, expect, test } from 'bun:test';
import { isVisibleCleanupProfile, readCleanupProfileInventory, resolveCurrentCleanupProfile } from './cleanupProfileIdentity';

describe('cleanup account identity', () => {
  test('distinguishes an empty account list from failed or unavailable inventory', async () => {
    expect(await readCleanupProfileInventory(async () => ({ success: true, data: { profiles: [] } })))
      .toEqual({ ok: true, data: { profiles: [] } });
    expect(await readCleanupProfileInventory(async () => ({ success: false, data: null }))).toEqual({ ok: false });
    expect(await readCleanupProfileInventory(async () => { throw new Error('unavailable'); })).toEqual({ ok: false });
    expect(await readCleanupProfileInventory(async () => ({ success: true, data: null }))).toEqual({ ok: false });
  });
  test('keeps real accounts regardless of words in their names or paths', () => {
    expect(isVisibleCleanupProfile({ name: 'SandboxAdmin', path: 'C:\\Users\\SandboxAdmin' })).toBe(true);
    expect(isVisibleCleanupProfile({ name: 'Codex', displayName: 'Alex Codex', path: 'C:\\Users\\Codex' })).toBe(true);
    expect(isVisibleCleanupProfile({ name: 'Alice', path: 'D:\\codex\\Users\\Alice' })).toBe(true);
    expect(isVisibleCleanupProfile({ name: 'Public', path: 'C:\\Users\\Public' })).toBe(false);
    expect(isVisibleCleanupProfile({ name: 'Alias', path: 'C:\\Users\\Public\\' })).toBe(false);
    expect(isVisibleCleanupProfile({ name: 'Default', path: 'C:\\Users\\Default' })).toBe(false);
  });
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
