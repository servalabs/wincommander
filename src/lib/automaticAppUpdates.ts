import type { AppAutoUpdateAttempt, PendingUpdateEntry } from "../types/settings";

/** An automatic package update gets its initial try plus exactly one retry. */
export const MAX_AUTOMATIC_APP_UPDATE_ATTEMPTS = 2;

function normalizedVersion(update: PendingUpdateEntry): string | null {
  const version = update.latestVersion?.trim();
  return version || null;
}

/**
 * Select upgrades that are safe to start automatically. The retry record is
 * version-specific: a newly discovered version is eligible even if an older
 * version exhausted its two attempts.
 */
export function getAutomaticAppUpdateCandidates({
  pendingUpdates,
  ignoredFindingIds,
  attemptsById,
  manifestOnly,
}: {
  pendingUpdates: PendingUpdateEntry[];
  ignoredFindingIds: readonly string[];
  attemptsById: Record<string, AppAutoUpdateAttempt>;
  manifestOnly: boolean;
}): PendingUpdateEntry[] {
  const ignored = new Set(ignoredFindingIds);
  const seen = new Set<string>();

  return pendingUpdates.filter((update) => {
    const id = update.id.trim();
    if (!id || seen.has(id)) return false;
    seen.add(id);
    if (ignored.has(`app-update:${id}`)) return false;
    if (manifestOnly && !update.inManifest) return false;

    const previous = attemptsById[id];
    const sameAvailableVersion = previous?.version === normalizedVersion(update);
    return !sameAvailableVersion || previous.attempts < MAX_AUTOMATIC_APP_UPDATE_ATTEMPTS;
  });
}

/** Return the durable record immediately before starting one package attempt. */
export function recordAutomaticAppUpdateAttempt(
  attemptsById: Record<string, AppAutoUpdateAttempt>,
  update: PendingUpdateEntry,
): Record<string, AppAutoUpdateAttempt> {
  const id = update.id.trim();
  if (!id) return attemptsById;
  const version = normalizedVersion(update);
  const previous = attemptsById[id];
  const attempts = previous?.version === version ? previous.attempts + 1 : 1;
  return { ...attemptsById, [id]: { version, attempts } };
}

/** A successful/no-longer-applicable command clears its failure history. */
export function clearAutomaticAppUpdateAttempt(
  attemptsById: Record<string, AppAutoUpdateAttempt>,
  id: string,
): Record<string, AppAutoUpdateAttempt> {
  const next = { ...attemptsById };
  delete next[id.trim()];
  return next;
}
