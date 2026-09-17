import type { CleanupCategory } from "./cleanupCategories";

/**
 * The defaults behind "Auto-set scheduled wipes".  Keep this separate from
 * the scheduler transport: changing the cadence is a product-policy decision,
 * not a Windows Scheduled Task implementation detail.
 */
export const AUTO_SCHEDULE_INTERVALS = {
  highFrequency: 60,
  manyFindings: 180,
  normal: 360,
  heavy: 1440,
} as const;

const HIGH_FREQUENCY_PRIVACY_IDS = new Set([
  "clipboardHistory", "clipboard", "dnsCache", "recentFiles", "jumpLists",
  "psHistory", "rdpHistory", "browserFootprints", "shellBags", "netDrives",
]);

/** A large result set is a useful signal to clean ordinary traces sooner. */
export const MANY_FINDINGS_THRESHOLD = 100;

export function getAutoScheduleInterval(
  category: Pick<CleanupCategory, "id" | "group" | "systemWide" | "minIntervalMinutes">,
  findings: number | undefined,
): number {
  const requested = category.group === "deep-dfir" || category.systemWide
    ? AUTO_SCHEDULE_INTERVALS.heavy
    : HIGH_FREQUENCY_PRIVACY_IDS.has(category.id)
      ? AUTO_SCHEDULE_INTERVALS.highFrequency
      : (findings ?? 0) >= MANY_FINDINGS_THRESHOLD
        ? AUTO_SCHEDULE_INTERVALS.manyFindings
        : AUTO_SCHEDULE_INTERVALS.normal;

  return Math.max(category.minIntervalMinutes ?? 1, requested);
}
