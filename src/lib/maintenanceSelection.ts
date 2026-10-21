import type { DuplicateGroup } from "../hooks/useBackend";

export type DuplicateKeepPolicy = "first" | "shortest-path";

export function getRetainedFileId(group: DuplicateGroup, policy: DuplicateKeepPolicy, overrides: Record<string, string> = {}): string | undefined {
  const override = overrides[group.id];
  if (group.files.some((file) => file.id === override)) return override;
  if (policy === "shortest-path") return [...group.files].sort((left, right) => left.path.length - right.path.length || left.path.localeCompare(right.path))[0]?.id;
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
