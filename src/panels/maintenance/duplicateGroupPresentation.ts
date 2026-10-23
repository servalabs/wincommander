/** `null` means every loaded group is expanded; a set records individual opens. */
export type ExpandedDuplicateGroups = ReadonlySet<string> | null;

export function isDuplicateGroupExpanded(expanded: ExpandedDuplicateGroups, groupId: string) {
  return expanded === null || expanded.has(groupId);
}

export function hasExpandedDuplicateGroups(expanded: ExpandedDuplicateGroups) {
  return expanded === null || expanded.size > 0;
}

export function setDuplicateGroupExpanded(
  expanded: ExpandedDuplicateGroups,
  loadedGroupIds: readonly string[],
  groupId: string,
  open: boolean,
): Set<string> {
  const next = expanded === null ? new Set(loadedGroupIds) : new Set(expanded);
  if (open) next.add(groupId);
  else next.delete(groupId);
  return next;
}

export function toggleDuplicateGroups(expanded: ExpandedDuplicateGroups) {
  return hasExpandedDuplicateGroups(expanded) ? new Set<string>() : null;
}
