import { expect, test } from "bun:test";
import { isVerifiedIndividualSchedule, isVerifiedManagedSchedule, readCleanupScheduleSnapshot, type CleanupScheduleRow } from "./cleanupScheduleReadback";

const relevant = new Set(["dnsCache", "amcache"]);
const ready: CleanupScheduleRow = { categoryId: "dnsCache", enabled: true, state: "Ready", scope: "current-user", intervalMinutes: 60, targetUser: null, managedByAutoSet: true };

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
