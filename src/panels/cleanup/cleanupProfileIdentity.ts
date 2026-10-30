/** Do not label an unrelated account as the current user if inventory is incomplete. */
export function resolveCurrentCleanupProfile(profiles: Array<{ name: string; isCurrent?: boolean }>, currentUser?: string | null): string {
  const marked = profiles.filter(profile => profile.isCurrent === true);
  if (marked.length === 1) return marked[0].name;
  if (marked.length > 1 || !currentUser) return '';
  const matching = profiles.filter(profile => profile.name.toLocaleLowerCase() === currentUser.toLocaleLowerCase());
  return matching.length === 1 && matching[0].isCurrent !== false ? matching[0].name : '';
}
