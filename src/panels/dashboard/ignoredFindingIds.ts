/**
 * Combines the saved ignore list with clicks that have not reached the settings
 * service yet. Keeping this pure makes the dashboard update at click time,
 * rather than waiting for the serialized settings write to complete.
 */
export function effectiveIgnoredFindingIds(
  savedIds: readonly string[],
  pendingAddedIds: readonly string[],
  pendingRestoredIds: readonly string[] = [],
): string[] {
  const restored = new Set(pendingRestoredIds);
  return [...new Set([...savedIds, ...pendingAddedIds])]
    .filter((id) => !restored.has(id));
}

/** Build a write-time patch so rapid Ignore clicks always union with the
 * newest saved list, rather than overwriting an earlier click's ID. */
export function addIgnoredFindingId(
  savedIds: readonly string[] | undefined,
  id: string,
): string[] {
  return effectiveIgnoredFindingIds(savedIds ?? [], [id]);
}

export function removeIgnoredFindingId(
  savedIds: readonly string[] | undefined,
  id: string,
): string[] {
  return (savedIds ?? []).filter((savedId) => savedId !== id);
}
