import { expect, test } from "bun:test";
import { canShowCleanupScheduleForSelection, isVerifiedIndividualSchedule, isVerifiedManagedSchedule, readCleanupScheduleSnapshot, type CleanupScheduleRow } from "./cleanupScheduleReadback";

const relevant = new Set(["dnsCache", "amcache"]);
const ready: CleanupScheduleRow = { categoryId: "dnsCache", enabled: true, state: "Ready", scope: "current-user", intervalMinutes: 60, targetUser: null, managedByAutoSet: true, payloadVerified: true };

test("keeps an unverified task manageable without reporting its payload as verified", () => {
  for (const payloadVerified of [false, undefined]) {
    const snapshot = readCleanupScheduleSnapshot([{ ...ready, managedByAutoSet: false, payloadVerified }], relevant)!;
    expect(snapshot.schedules.dnsCache).toBe(60);
    expect(isVerifiedIndividualSchedule(snapshot.records.dnsCache, 60, 60, "current-user")).toBe(false);
  }
});

test("keeps an enabled task with an unreadable interval available for removal", () => {
  const broken = { ...ready, intervalMinutes: 0, payloadVerified: false, managedByAutoSet: false };
  const snapshot = readCleanupScheduleSnapshot([broken, { ...ready, categoryId: "amcache" }], relevant)!;
  expect(snapshot.records.dnsCache.enabled).toBe(true);
  expect(snapshot.records.dnsCache.intervalMinutes).toBeNull();
  expect(snapshot.schedules.dnsCache).toBeUndefined();
  expect(snapshot.disabledSchedules.dnsCache).toBeUndefined();
  expect(snapshot.schedules.amcache).toBe(60);
  expect(isVerifiedIndividualSchedule(snapshot.records.dnsCache, 60, 60, "current-user")).toBe(false);
  expect(readCleanupScheduleSnapshot([{ ...broken, payloadVerified: true }], relevant)).toBeNull();
});

test("accepts only verified state and verifies the expected interval and owner", () => {
  const snapshot = readCleanupScheduleSnapshot([ready], relevant)!;
  expect(snapshot.schedules.dnsCache).toBe(60);
  expect(isVerifiedManagedSchedule(snapshot.records.dnsCache, 60, "current-user")).toBe(true);
  expect(isVerifiedManagedSchedule(snapshot.records.dnsCache, 15, "current-user")).toBe(false);
  expect(isVerifiedManagedSchedule(snapshot.records.dnsCache, 60, "system")).toBe(false);
  expect(isVerifiedIndividualSchedule(snapshot.records.dnsCache, 60, 60, "current-user")).toBe(false);
  const manual = { ...snapshot.records.dnsCache, managedByAutoSet: false };
  expect(isVerifiedIndividualSchedule(manual, 60, 60, "current-user")).toBe(true);
  expect(isVerifiedIndividualSchedule(manual, 60, 15, "current-user")).toBe(false);
  expect(readCleanupScheduleSnapshot([{ ...ready, state: "Unknown", enabled: false }], relevant)).toBeNull();
  expect(readCleanupScheduleSnapshot([{ ...ready, state: undefined }], relevant)).toBeNull();
  expect(readCleanupScheduleSnapshot([{ ...ready, enabled: false }], relevant)).toBeNull();
});

test("preserves a disabled task separately and rejects duplicate local owners in either order", () => {
  const disabled = { ...ready, state: "Disabled", enabled: false, intervalMinutes: 60 };
  const snapshot = readCleanupScheduleSnapshot([disabled], relevant)!;
  expect(snapshot.schedules.dnsCache).toBeUndefined();
  expect(snapshot.disabledSchedules.dnsCache.intervalMinutes).toBe(60);
  expect(snapshot.managedIds.has("dnsCache")).toBe(true);
  expect(isVerifiedManagedSchedule(snapshot.records.dnsCache, 60, "current-user")).toBe(false);
  const system = { ...ready, scope: "system" as const };
  expect(readCleanupScheduleSnapshot([ready, system], relevant)).toBeNull();
  expect(readCleanupScheduleSnapshot([system, ready], relevant)).toBeNull();
});

test("uses the verified canonical task when disabled legacy tasks remain", () => {
  const canonical = { ...ready, legacy: false, managedByAutoSet: false };
  const old = { ...ready, legacy: true, state: "Disabled", enabled: false };
  for (const rows of [[canonical, old], [old, canonical]]) {
    const snapshot = readCleanupScheduleSnapshot(rows, relevant)!;
    expect(snapshot.schedules.dnsCache).toBe(60);
    expect(snapshot.disabledSchedules.dnsCache).toBeUndefined();
    expect(snapshot.managedIds.has("dnsCache")).toBe(false);
  }
  expect(readCleanupScheduleSnapshot([canonical, { ...old, enabled: true, state: "Ready" }], relevant)).toBeNull();
  expect(readCleanupScheduleSnapshot([canonical, { ...old, scope: "system" }], relevant)).toBeNull();
  expect(readCleanupScheduleSnapshot([{ ...canonical, legacy: undefined }, old], relevant)).toBeNull();
  expect(readCleanupScheduleSnapshot([{ ...old, legacy: undefined }, canonical], relevant)).toBeNull();
  expect(readCleanupScheduleSnapshot([old, { ...old }], relevant)).toBeNull();
});


test("only displays task history for the selected verified local task", () => {
  const canonical = { ...ready, legacy: false, lastRun: "2026-10-30T10:30:00Z", lastResult: 0, nextRun: "2026-10-30T11:30:00Z" };
  const old = { ...ready, legacy: true, state: "Disabled", enabled: false, lastResult: 1 };
  const other = { ...ready, scope: "other-user" as const, targetUser: "Other", lastResult: 2 };
  expect(readCleanupScheduleSnapshot([old, canonical, other], relevant)!.records.dnsCache.lastResult).toBe(0);
  expect(readCleanupScheduleSnapshot([canonical, { ...canonical, scope: "system" }], relevant)).toBeNull();
});


test("never attributes current-user task history to another account", () => {
  const own = readCleanupScheduleSnapshot([ready], relevant)!.records.dnsCache;
  expect(canShowCleanupScheduleForSelection(true, false, own)).toBe(true);
  expect(canShowCleanupScheduleForSelection(false, false, own)).toBe(false);
  expect(canShowCleanupScheduleForSelection(false, true, own)).toBe(false);
  expect(canShowCleanupScheduleForSelection(false, true, { ...own, scope: "system" })).toBe(true);
  expect(canShowCleanupScheduleForSelection(false, true, undefined)).toBe(false);
});

test("keeps only well-formed cleanup outcomes for the verified local task", () => {
  const outcome = { attemptedUtc: "2026-10-30T10:31:00Z", success: false, removed: 2, deferred: 0, failed: 1 };
  const row = { ...ready, lastRun: "2026-10-30T10:30:00Z", cleanupOutcome: outcome };
  expect(readCleanupScheduleSnapshot([row], relevant)!.records.dnsCache.cleanupOutcome).toEqual(outcome);
  expect(readCleanupScheduleSnapshot([{ ...row, lastRun: null }], relevant)!.records.dnsCache.cleanupOutcome).toBeNull();
  expect(readCleanupScheduleSnapshot([{ ...row, cleanupOutcome: { ...outcome, success: true } }], relevant)!.records.dnsCache.cleanupOutcome).toBeNull();
  expect(readCleanupScheduleSnapshot([{ ...row, cleanupOutcome: { ...outcome, removed: -1 } }], relevant)!.records.dnsCache.cleanupOutcome).toBeNull();
  expect(readCleanupScheduleSnapshot([{ ...row, scope: "other-user", targetUser: "Other" }], relevant)!.records.dnsCache).toBeUndefined();
});
