// The backend identifies user profiles by Windows SID and excludes special
// profiles. Only reject reserved template folders here; account names are
// user-controlled and may legitimately contain words like "sandbox".
export function isVisibleCleanupProfile(profile: { name?: string; displayName?: string; path?: string }): boolean {
  const name = (profile.name ?? '').trim();
  if (!name) return false;
  const reserved = new Set(['default', 'default user', 'defaultuser0', 'public', 'all users', 'wdagutilityaccount']);
  const displayName = (profile.displayName ?? '').trim().toLowerCase();
  const path = (profile.path ?? '').replace(/[\\/]+$/, '').toLowerCase();
  return !reserved.has(name.toLowerCase()) && !reserved.has(displayName) &&
    !path.endsWith('\\users\\default') && !path.endsWith('\\users\\public');
}

/** Distinguish an unavailable inventory from a successfully read empty list. */
export async function readCleanupProfileInventory<T>(
  load: () => Promise<{ success: boolean; data?: T | null }>,
): Promise<{ ok: true; data: T } | { ok: false }> {
  try {
    const result = await load();
    return result.success && result.data != null ? { ok: true, data: result.data } : { ok: false };
  } catch {
    return { ok: false };
  }
}

/** Do not label an unrelated account as the current user if inventory is incomplete. */
export function resolveCurrentCleanupProfile(profiles: Array<{ name: string; isCurrent?: boolean }>, currentUser?: string | null): string {
  const marked = profiles.filter(profile => profile.isCurrent === true);
  if (marked.length === 1) return marked[0].name;
  if (marked.length > 1 || !currentUser) return '';
  const matching = profiles.filter(profile => profile.name.toLocaleLowerCase() === currentUser.toLocaleLowerCase());
  return matching.length === 1 && matching[0].isCurrent !== false ? matching[0].name : '';
}
