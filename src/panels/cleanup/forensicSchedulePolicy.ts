import { SCHEDULABLE_CATEGORIES, type CleanupCategory } from "./cleanupCategories";
import { getAutoScheduleInterval } from "./autoSchedulePolicy";
import { isAutomaticCleanupExcluded, isCleanupCommandSuccessful } from "./cleanupManualPolicy";
import { isVerifiedManagedSchedule, readCleanupScheduleSnapshot, type CleanupScheduleRow, type CleanupScheduleScope } from "./cleanupScheduleReadback";

const NO_ADDITIONAL_EXCLUSIONS = new Set<string>();

export interface AutoEraseScheduleSnapshot {
  categoryId: string;
  enabled: boolean;
  intervalMinutes: number;
  targetUser: string | null;
}

export interface ForensicScheduleSummary {
  created: number;
  alreadyConfigured: number;
  disabledExisting: number;
  skipped: number;
  failed: number;
  firstFailure?: string;
}

export type AutoEraseScheduleWriter = (
  categoryId: string,
  intervalMinutes: number,
  runAsSystem: boolean,
) => Promise<{ success: boolean; error?: string; data?: { status?: string; taskState?: string } }>;
export type AutoEraseScheduleReader = () => Promise<{ success: boolean; error?: string; data?: { schedules: CleanupScheduleRow[] } }>;

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
      && !FORENSIC_SCHEDULE_PROTECTED_IDS.has(schedulerId)
      && !isAutomaticCleanupExcluded(category.id, NO_ADDITIONAL_EXCLUSIONS)
      && !isAutomaticCleanupExcluded(schedulerId, NO_ADDITIONAL_EXCLUSIONS);
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

/** Uses the managed-only backend command, which preserves existing schedules. */
export async function ensureForensicTraceSchedules(
  writeSchedule: AutoEraseScheduleWriter,
  readSchedules: AutoEraseScheduleReader,
  categories: CleanupCategory[] = getForensicTraceScheduleCategories(),
): Promise<ForensicScheduleSummary> {
  const eligibleCategories = getForensicTraceScheduleCategories(categories);
  const summary: ForensicScheduleSummary = {
    created: 0,
    alreadyConfigured: 0,
    disabledExisting: 0,
    skipped: categories.length - eligibleCategories.length,
    failed: 0,
  };
  const created: Array<{ id: string; interval: number; scope: CleanupScheduleScope }> = [];
  const existingActive: string[] = [];
  const existingDisabled: string[] = [];

  for (const category of eligibleCategories) {
    if (!category.schedulable || !category.clearDataKey) {
      summary.skipped++;
      continue;
    }

    try {
      const interval = getAutoScheduleInterval(category, undefined);
      const result = await writeSchedule(
        category.schedulerCategoryId ?? category.id,
        interval,
        category.schedulerRunAsSystem === true,
      );
      if (isCleanupCommandSuccessful(result) && result.data?.status === "alreadyConfigured") {
        if (result.data.taskState === "Disabled") { summary.disabledExisting++; existingDisabled.push(category.id); }
        else if (["Ready", "Running", "Queued"].includes(result.data.taskState ?? "")) { summary.alreadyConfigured++; existingActive.push(category.id); }
        else { summary.failed++; summary.firstFailure ??= `Existing schedule status for ${category.label} is unknown.`; }
      } else if (isCleanupCommandSuccessful(result)) {
        summary.created++;
        created.push({ id: category.id, interval, scope: category.schedulerRunAsSystem ? "system" : "current-user" });
      } else {
        summary.failed++;
        summary.firstFailure ??= result.error || `Failed to schedule ${category.label}.`;
      }
    } catch (error) {
      summary.failed++;
      summary.firstFailure ??= error instanceof Error ? error.message : String(error);
    }
  }

  if (created.length || existingActive.length || existingDisabled.length) {
    let snapshot: ReturnType<typeof readCleanupScheduleSnapshot> = null;
    try {
      const readback = await readSchedules();
      if (isCleanupCommandSuccessful(readback) && Array.isArray(readback.data?.schedules)) {
        snapshot = readCleanupScheduleSnapshot(readback.data.schedules, new Set(eligibleCategories.map(category => category.id)));
      }
    } catch { /* An unavailable readback cannot verify a schedule. */ }
    for (const { id, interval, scope } of created) {
      if (!snapshot || !isVerifiedManagedSchedule(snapshot.records[id], interval, scope)) {
        summary.created--; summary.failed++; summary.firstFailure ??= "A created schedule could not be verified.";
      }
    }
    for (const id of existingActive) {
      if (!snapshot?.records[id]?.enabled || snapshot.records[id].payloadVerified !== true) {
        summary.alreadyConfigured--; summary.failed++; summary.firstFailure ??= "An existing schedule or its cleanup script could not be verified.";
      }
    }
    for (const id of existingDisabled) {
      if (!snapshot?.records[id] || snapshot.records[id].enabled) {
        summary.disabledExisting--; summary.failed++; summary.firstFailure ??= "An existing schedule could not be verified as disabled.";
      }
    }
  }

  return summary;
}
