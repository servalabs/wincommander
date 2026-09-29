export const LICENSE_REFRESH_BASE_MS = 4 * 60 * 60 * 1_000;
export const LICENSE_REFRESH_INITIAL_DELAY_MS = 1_500;
const LICENSE_REFRESH_DUE_JITTER_MS = 5 * 60 * 1_000;
const RETRY_DELAYS_MS = [15 * 60 * 1_000, 60 * 60 * 1_000, 4 * 60 * 60 * 1_000];

/**
 * Returns the delay until the persisted native verification reaches its
 * four-hour deadline. Jitter is only added after the deadline, never before it,
 * so an app started three hours and fifty-nine minutes after verification
 * checks again in about one minute rather than another four hours.
 */
export function nextLicenseRefreshDueDelay(
  lastVerifiedAtSeconds: number | null | undefined,
  nowMs: number = Date.now(),
  random: () => number = Math.random,
): number {
  if (!Number.isFinite(lastVerifiedAtSeconds)) {
    return LICENSE_REFRESH_INITIAL_DELAY_MS;
  }

  const verifiedAtMs = (lastVerifiedAtSeconds as number) * 1_000;
  if (verifiedAtMs > nowMs) {
    return LICENSE_REFRESH_INITIAL_DELAY_MS;
  }
  const dueAtMs = verifiedAtMs + LICENSE_REFRESH_BASE_MS;
  const remainingMs = dueAtMs - nowMs;
  if (remainingMs <= 0) {
    return LICENSE_REFRESH_INITIAL_DELAY_MS;
  }

  const jitterMs = Math.max(0, Math.min(1, random())) * LICENSE_REFRESH_DUE_JITTER_MS;
  return Math.max(LICENSE_REFRESH_INITIAL_DELAY_MS, remainingMs + jitterMs);
}

/** Retries only after a due native refresh actually fails. */
export function nextLicenseRefreshRetryDelay(failures: number): number {
  return RETRY_DELAYS_MS[Math.min(Math.max(failures, 1) - 1, RETRY_DELAYS_MS.length - 1)];
}
