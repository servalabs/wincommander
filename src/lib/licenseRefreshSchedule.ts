export const LICENSE_REFRESH_BASE_MS = 12 * 60 * 60 * 1_000;
const LICENSE_REFRESH_JITTER_MS = 60 * 60 * 1_000;
const RETRY_DELAYS_MS = [15 * 60 * 1_000, 60 * 60 * 1_000, 4 * 60 * 60 * 1_000];

/**
 * Keeps ordinary online validation near twice per day, while spreading client
 * requests and retrying transient failures without a rapid retry loop.
 */
export function nextLicenseRefreshDelay(failures: number, random: () => number = Math.random): number {
  if (failures > 0) {
    return RETRY_DELAYS_MS[Math.min(failures - 1, RETRY_DELAYS_MS.length - 1)];
  }
  const jitter = (Math.max(0, Math.min(1, random())) * 2 - 1) * LICENSE_REFRESH_JITTER_MS;
  return LICENSE_REFRESH_BASE_MS + jitter;
}
