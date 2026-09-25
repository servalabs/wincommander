import { SCHEDULABLE_CATEGORIES, type CleanupCategory } from "./cleanupCategories";
import { getAutoScheduleInterval } from "./autoSchedulePolicy";

export interface AutoEraseScheduleSnapshot {
  categoryId: string;
  enabled: boolean;
  intervalMinutes: number;
  targetUser: string | null;
}

export interface ForensicScheduleSummary {
  created: number;
  alreadyConfigured: number;
  skipped: number;
  failed: number;
  firstFailure?: string;
}

export type AutoEraseScheduleWriter = (
  categoryId: string,
  intervalMinutes: number,
  runAsSystem: boolean,
  preserveExisting: boolean,
  managedByAutoSet: boolean,
) => Promise<{ success: boolean; error?: string; data?: { status?: string } }>;

// The auto-schedule path is deliberately narrower than Cleanup's category
// catalogue. These are the only recurring clears reviewed as forensic traces;
// adding a new schedulable category requires an explicit safety review here.
export const FORENSIC_SCHEDULE_ALLOWED_IDS = new Set([
  "shellBags",
  "usbHistory",
  "dnsCache",
  "execCache",
  "eventLogs",
  "psHistory",
  "recentFiles",
  "prefetchFiles",
  "ntfsJournals",
  "pcaDatabase",
  "eventTranscript",
  "activitiesTimeline",
  "servicingLogs",
  "deviceInstallLogs",
  "usageTraceLogs",
  "defenderHistory",
  "appLaunchHistory",
  "officeMru",
  "reliabilityHistory",
  "explorerSearchHistory",
]);

// Protected by Cleanup's default exclusions or contain user content, saved
// credentials, persistent app state, or rebuildable content caches. Keep both
// the UI category id and scheduler id represented where they differ.
export const FORENSIC_SCHEDULE_PROTECTED_IDS = new Set([
  "recycleBin",
  "clipboardHistory",
  "clipboard",
  "wlanProfiles",
  "wslData",
  "dockerDesktopData",
  "virtualMachineArtifacts",
  "developerCaches",
  "credentialManager",
  "sshState",
  "passwordManagerCaches",
  "netDrives",
  "rdpHistory",
  "browserFootprints",
  "shadowCopies",
  "notepadState",
  "crashDumps",
  "walFiles",
  "recallDb",
  "searchIndex",
  "printSpooler",
  "webCache",
  "thumbnailDb",
  "notificationDb",
  "branchCache",
  "embeddedWebCache",
  "p2pUpdateCache",
  "searchPersonalization",
  "jumpLists",
  "rdpBitmapCache",
]);

export function getForensicTraceScheduleCategories(
  categories: CleanupCategory[] = SCHEDULABLE_CATEGORIES,
): CleanupCategory[] {
  return categories.filter((category) => {
    const schedulerId = category.schedulerCategoryId ?? category.id;
    return category.schedulable === true
      && Boolean(category.clearDataKey)
      && FORENSIC_SCHEDULE_ALLOWED_IDS.has(schedulerId)
      && category.usabilityTier !== "data-accounts-recovery"
      && !FORENSIC_SCHEDULE_PROTECTED_IDS.has(category.id)
      && !FORENSIC_SCHEDULE_PROTECTED_IDS.has(schedulerId);
  });
}

export function getMissingForensicTraceScheduleCategories(
  schedules: AutoEraseScheduleSnapshot[],
  categories: CleanupCategory[] = getForensicTraceScheduleCategories(),
): CleanupCategory[] {
  // A disabled task is an existing user choice. Preserve it instead of
  // repeatedly recommending a bulk action whose backend correctly leaves it
  // untouched under PreserveExisting.
  const configuredScheduleIds = new Set(
    schedules
      .filter((schedule) => !schedule.targetUser)
      .map((schedule) => schedule.categoryId.trim().toLowerCase()),
  );

  return categories.filter((category) =>
    !configuredScheduleIds.has((category.schedulerCategoryId ?? category.id).toLowerCase()),
  );
}

/** Adds only missing trace schedules; preserveExisting keeps manual cadences intact. */
export async function ensureForensicTraceSchedules(
  writeSchedule: AutoEraseScheduleWriter,
  categories: CleanupCategory[] = getForensicTraceScheduleCategories(),
): Promise<ForensicScheduleSummary> {
  const eligibleCategories = getForensicTraceScheduleCategories(categories);
  const summary: ForensicScheduleSummary = {
    created: 0,
    alreadyConfigured: 0,
    skipped: categories.length - eligibleCategories.length,
    failed: 0,
  };

  for (const category of eligibleCategories) {
    if (!category.schedulable || !category.clearDataKey) {
      summary.skipped++;
      continue;
    }

    try {
      const result = await writeSchedule(
        category.schedulerCategoryId ?? category.id,
        getAutoScheduleInterval(category, undefined),
        category.schedulerRunAsSystem === true,
        true,
        true,
      );
      if (result.success && result.data?.status === "alreadyConfigured") {
        summary.alreadyConfigured++;
      } else if (result.success) {
        summary.created++;
      } else {
        summary.failed++;
        summary.firstFailure ??= result.error || `Failed to schedule ${category.label}.`;
      }
    } catch (error) {
      summary.failed++;
      summary.firstFailure ??= error instanceof Error ? error.message : String(error);
    }
  }

  return summary;
}
