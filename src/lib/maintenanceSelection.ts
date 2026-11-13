import type { DuplicateGroup } from "../hooks/useBackend";

export type DuplicateKeepPolicy = "first" | "shortest-path" | "newest-modified" | "oldest-modified";

// Git keeps content-addressed objects under .git. Even if an older scan
// marked such a file removable, it must not be selected for deletion.
export function isGitManagedDuplicate(path: string): boolean {
  return /(?:^|[\\/])\.git(?:[\\/]|$)/i.test(path);
}

function modifiedNanos(value: string | null): bigint | undefined {
  if (!value || !/^\d+$/.test(value)) return undefined;
  try { return BigInt(value); } catch { return undefined; }
}

function compareStable(left: DuplicateGroup["files"][number], right: DuplicateGroup["files"][number]) {
  return left.path.localeCompare(right.path) || left.id.localeCompare(right.id);
}

function compareByModified(left: DuplicateGroup["files"][number], right: DuplicateGroup["files"][number], newest: boolean) {
  const leftModified = modifiedNanos(left.modified);
  const rightModified = modifiedNanos(right.modified);
  if (leftModified === undefined && rightModified === undefined) return compareStable(left, right);
  if (leftModified === undefined) return 1;
  if (rightModified === undefined) return -1;
  if (leftModified === rightModified) return compareStable(left, right);
  return newest
    ? (leftModified > rightModified ? -1 : 1)
    : (leftModified < rightModified ? -1 : 1);
}

export function getRetainedFileId(group: DuplicateGroup, policy: DuplicateKeepPolicy, overrides: Record<string, string> = {}): string | undefined {
  // Prefer retaining an ordinary file over a hidden Git object. A Git object
  // may contain identical bytes, but it is not a replacement for the user's
  // visible file in its original location.
  const ordinaryFiles = group.files.filter((file) => !isGitManagedDuplicate(file.path));
  const candidates = ordinaryFiles.length ? ordinaryFiles : group.files;
  const override = overrides[group.id];
  if (candidates.some((file) => file.id === override)) return override;
  if (policy === "shortest-path") return [...candidates].sort((left, right) => left.path.length - right.path.length || left.path.localeCompare(right.path))[0]?.id;
  if (policy === "newest-modified") return [...candidates].sort((left, right) => compareByModified(left, right, true))[0]?.id;
  if (policy === "oldest-modified") return [...candidates].sort((left, right) => compareByModified(left, right, false))[0]?.id;
  return candidates[0]?.id;
}

export function canRemoveDuplicateFile(groups: DuplicateGroup[], id: string, policy: DuplicateKeepPolicy, overrides: Record<string, string> = {}): boolean {
  return groups.some(group => group.files.some(file => file.id === id && file.removable === true && !isGitManagedDuplicate(file.path)) && getRetainedFileId(group, policy, overrides) !== id);
}

export function selectDuplicateCopies(groups: DuplicateGroup[], policy: DuplicateKeepPolicy, overrides: Record<string, string> = {}): Set<string> {
  return new Set(groups.flatMap((group) => {
    const retainedId = getRetainedFileId(group, policy, overrides);
    return group.files.filter((file) => file.id !== retainedId && file.removable === true && !isGitManagedDuplicate(file.path)).map((file) => file.id);
  }));
}

export function getDuplicateSelectedBytes(groups: DuplicateGroup[], selected: Set<string>): number {
  const files = new Map(groups.flatMap((group) => group.files.map((file) => [file.id, file] as const)));
  return [...selected].reduce((sum, id) => sum + (files.get(id)?.size ?? 0), 0);
}
