import type { DuplicateGroup } from "../hooks/useBackend";

export type DuplicateKeepPolicy = "first" | "shortest-path" | "newest-modified" | "oldest-modified";

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
  const override = overrides[group.id];
  if (group.files.some((file) => file.id === override)) return override;
  if (policy === "shortest-path") return [...group.files].sort((left, right) => left.path.length - right.path.length || left.path.localeCompare(right.path))[0]?.id;
  if (policy === "newest-modified") return [...group.files].sort((left, right) => compareByModified(left, right, true))[0]?.id;
  if (policy === "oldest-modified") return [...group.files].sort((left, right) => compareByModified(left, right, false))[0]?.id;
  return group.files[0]?.id;
}

export function selectDuplicateCopies(groups: DuplicateGroup[], policy: DuplicateKeepPolicy, overrides: Record<string, string> = {}): Set<string> {
  return new Set(groups.flatMap((group) => {
    const retainedId = getRetainedFileId(group, policy, overrides);
    return group.files.filter((file) => file.id !== retainedId).map((file) => file.id);
  }));
}

export function getDuplicateSelectedBytes(groups: DuplicateGroup[], selected: Set<string>): number {
  const files = new Map(groups.flatMap((group) => group.files.map((file) => [file.id, file] as const)));
  return [...selected].reduce((sum, id) => sum + (files.get(id)?.size ?? 0), 0);
}
