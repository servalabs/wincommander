import type { AppAutoUpdateAttempt, PendingUpdateEntry } from "../types/settings";

/** An automatic package update gets its initial try plus exactly one retry. */
export const MAX_AUTOMATIC_APP_UPDATE_ATTEMPTS = 2;

// This is a second guard in addition to the durable settings record.  A
// settings write is normally made before every attempt, but a stale renderer
// or a competing settings write must never turn a failed package into a hot
// retry loop.  Manual updates do not consult this map.
const sessionAttemptsById = new Map<string, AppAutoUpdateAttempt>();

function normalizedVersion(update: PendingUpdateEntry): string | null {
  const version = update.latestVersion?.trim();
  return version || null;
}

function attemptFor(
  attemptsById: Record<string, AppAutoUpdateAttempt>,
  update: PendingUpdateEntry,
): AppAutoUpdateAttempt | undefined {
  const id = update.id.trim();
  const version = normalizedVersion(update);
  const persisted = attemptsById[id];
  const session = sessionAttemptsById.get(id);
  const persistedForVersion = persisted?.version === version ? persisted : undefined;
  const sessionForVersion = session?.version === version ? session : undefined;

  if (!persistedForVersion) return sessionForVersion;
  if (!sessionForVersion || persistedForVersion.attempts >= sessionForVersion.attempts) {
    return persistedForVersion;
  }
  return sessionForVersion;
}

/** The effective count includes this renderer's non-persisted safety guard. */
export function getAutomaticAppUpdateAttemptCount(
  attemptsById: Record<string, AppAutoUpdateAttempt>,
  update: PendingUpdateEntry,
): number {
  return attemptFor(attemptsById, update)?.attempts ?? 0;
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

    return getAutomaticAppUpdateAttemptCount(attemptsById, update) < MAX_AUTOMATIC_APP_UPDATE_ATTEMPTS;
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
  const previousAttempts = getAutomaticAppUpdateAttemptCount(attemptsById, update);
  const next = { version, attempts: Math.min(previousAttempts + 1, MAX_AUTOMATIC_APP_UPDATE_ATTEMPTS) };
  sessionAttemptsById.set(id, next);
  return { ...attemptsById, [id]: next };
}

/** A successful/no-longer-applicable command clears its failure history. */
export function clearAutomaticAppUpdateAttempt(
  attemptsById: Record<string, AppAutoUpdateAttempt>,
  id: string,
): Record<string, AppAutoUpdateAttempt> {
  sessionAttemptsById.delete(id.trim());
  const next = { ...attemptsById };
  delete next[id.trim()];
  return next;
}
