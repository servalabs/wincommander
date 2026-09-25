import { describe, expect, test } from "bun:test";
import { SCHEDULABLE_CATEGORIES } from "./cleanupCategories";
import {
  ensureForensicTraceSchedules,
  FORENSIC_SCHEDULE_ALLOWED_IDS,
  FORENSIC_SCHEDULE_PROTECTED_IDS,
  getForensicTraceScheduleCategories,
  getMissingForensicTraceScheduleCategories,
} from "./forensicSchedulePolicy";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

const DEFAULT_CLEANUP_EXCLUDED_IDS = new Set([
  "wlanProfiles",
  "browserFootprints",
  "notepadState",
  "wslData",
  "dockerDesktopData",
  "virtualMachineArtifacts",
  "developerCaches",
  "credentialManager",
  "sshState",
  "passwordManagerCaches",
]);

describe("Secure persona scheduled trace wipes", () => {
  test("keeps every default-excluded schedulable cleanup target out of the recommendation", async () => {
    const cleanup = await Bun.file("src/panels/cleanup/useCleanupScan.ts").text();
    const start = cleanup.indexOf("const DEFAULT_BULK_CLEAR_EXCLUDES = [");
    const end = cleanup.indexOf("];", start);
    expect(start).toBeGreaterThan(-1);
    const defaultExcludedIds = Array.from(
      cleanup.slice(start, end).matchAll(/'([^']+)'/g),
      (match) => match[1],
    );
    expect(defaultExcludedIds.sort()).toEqual([...DEFAULT_CLEANUP_EXCLUDED_IDS].sort());
    for (const id of defaultExcludedIds) {
      expect(FORENSIC_SCHEDULE_PROTECTED_IDS.has(id)).toBe(true);
    }
    expect(getForensicTraceScheduleCategories().some((category) =>
      defaultExcludedIds.includes(category.id),
    )).toBe(false);
    expect(getForensicTraceScheduleCategories().some((category) =>
      category.usabilityTier === "data-accounts-recovery",
    )).toBe(false);
  });

  test("schedules only reviewed trace categories and excludes user content", () => {
    const userContentAndAppStateIds = [
      "recycleBin",
      "clipboardHistory",
      "clipboard",
      "wlanProfiles",
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
    ];
    const scheduled = getForensicTraceScheduleCategories();

    expect(scheduled.length).toBeGreaterThan(0);
    for (const category of scheduled) {
      const schedulerId = category.schedulerCategoryId ?? category.id;
      expect(FORENSIC_SCHEDULE_ALLOWED_IDS.has(schedulerId)).toBe(true);
      expect(FORENSIC_SCHEDULE_PROTECTED_IDS.has(category.id)).toBe(false);
      expect(FORENSIC_SCHEDULE_PROTECTED_IDS.has(schedulerId)).toBe(false);
      expect(userContentAndAppStateIds.includes(category.id)).toBe(false);
      expect(userContentAndAppStateIds.includes(schedulerId)).toBe(false);
      expect(DEFAULT_CLEANUP_EXCLUDED_IDS.has(category.id)).toBe(false);
      expect(DEFAULT_CLEANUP_EXCLUDED_IDS.has(schedulerId)).toBe(false);
    }

    for (const id of userContentAndAppStateIds) {
      expect(FORENSIC_SCHEDULE_PROTECTED_IDS.has(id)).toBe(true);
      expect(scheduled.some((category) =>
        category.id === id || category.schedulerCategoryId === id,
      )).toBe(false);
    }
  });

  test("respects configured current-user schedules, including disabled and excludes other-user tasks", () => {
    const categories = getForensicTraceScheduleCategories().slice(0, 2);
    const [first, second] = categories;
    expect(first !== undefined && second !== undefined).toBe(true);

    const configured = getMissingForensicTraceScheduleCategories([
      {
        categoryId: first!.schedulerCategoryId ?? first!.id,
        enabled: true,
        intervalMinutes: 60,
        targetUser: null,
      },
      {
        categoryId: second!.schedulerCategoryId ?? second!.id,
        enabled: false,
        intervalMinutes: 60,
        targetUser: null,
      },
      {
        categoryId: second!.schedulerCategoryId ?? second!.id,
        enabled: true,
        intervalMinutes: 60,
        targetUser: "another-user",
      },
    ], categories);

    expect(configured).toEqual([]);
    const otherUserOnly = getMissingForensicTraceScheduleCategories([
      {
        categoryId: first!.schedulerCategoryId ?? first!.id,
        enabled: true,
        intervalMinutes: 60,
        targetUser: "another-user",
      },
    ], categories);
    expect(otherUserOnly.map((category) => category.id)).toEqual([first!.id, second!.id]);
  });

  test("applies schedules idempotently without overwriting an existing cadence", async () => {
    const categories = getForensicTraceScheduleCategories().slice(0, 2);
    const calls: Array<[string, number, boolean, boolean, boolean]> = [];
    const summary = await ensureForensicTraceSchedules(async (...args) => {
      calls.push(args);
      return calls.length === 1
        ? { success: true, data: { status: "created" } }
        : { success: true, data: { status: "alreadyConfigured" } };
    }, categories);

    expect(calls.length).toBe(2);
    expect(calls.every(([, , , preserveExisting, managedByAutoSet]) => preserveExisting && managedByAutoSet)).toBe(true);
    expect(summary).toEqual({ created: 1, alreadyConfigured: 1, skipped: 0, failed: 0 });
  });

  test("refuses protected categories passed directly to the scheduler helper", async () => {
    const rdpHistory = SCHEDULABLE_CATEGORIES.find((category) => category.id === "rdpHistory");
    expect(rdpHistory !== undefined).toBe(true);

    let writeCount = 0;
    const summary = await ensureForensicTraceSchedules(async () => {
      writeCount++;
      return { success: true, data: { status: "created" } };
    }, [rdpHistory!]);

    expect(writeCount).toBe(0);
    expect(summary).toEqual({ created: 0, alreadyConfigured: 0, skipped: 1, failed: 0 });
  });

  test("retains failures so Dashboard Fix All can report an unsuccessful schedule", async () => {
    const [category] = getForensicTraceScheduleCategories();
    expect(category !== undefined).toBe(true);
    const summary = await ensureForensicTraceSchedules(async () => ({ success: false, error: "Task Scheduler is unavailable." }), [category!]);

    expect(summary.failed).toBe(1);
    expect(summary.firstFailure).toBe("Task Scheduler is unavailable.");
  });
});
