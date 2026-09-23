import { describe, expect, test } from "bun:test";
import type { BackendResponse } from "../../hooks/useBackend";
import {
  createDiskCleanupScheduleCache,
  type DiskCleanupProfilesData,
  type DiskCleanupSchedulesData,
} from "./diskCleanupScheduleState";

const profiles: BackendResponse<DiskCleanupProfilesData> = {
  success: true,
  data: { profiles: [], total: 0, currentUser: "user", isAdmin: true },
};

const schedules: BackendResponse<DiskCleanupSchedulesData> = {
  success: true,
  data: { schedules: [], total: 0 },
};

describe("Maintenance disk-cleanup startup status cache", () => {
  test("startup preload and the card share in-flight reads and successful results", async () => {
    const cache = createDiskCleanupScheduleCache();
    let profileCalls = 0;
    let scheduleCalls = 0;
    let resolveSchedules: ((result: BackendResponse<DiskCleanupSchedulesData>) => void) | undefined;
    const getProfiles = async () => { profileCalls += 1; return profiles; };
    const getSchedules = () => {
      scheduleCalls += 1;
      return new Promise<BackendResponse<DiskCleanupSchedulesData>>((resolve) => { resolveSchedules = resolve; });
    };

    const preload = cache.preload({ getProfiles, getSchedules });
    const cardRead = cache.loadSchedules(getSchedules);
    await Promise.resolve();

    expect(profileCalls).toBe(1);
    expect(scheduleCalls).toBe(1);
    resolveSchedules?.(schedules);
    await preload;
    expect(await cardRead).toEqual(schedules);
    expect(await cache.loadSchedules(getSchedules)).toEqual(schedules);
    expect(scheduleCalls).toBe(1);
  });

  test("failed reads are shown to callers but retried instead of cached", async () => {
    const cache = createDiskCleanupScheduleCache();
    let calls = 0;
    const load = async (): Promise<BackendResponse<DiskCleanupSchedulesData>> =>
      ++calls === 1 ? { success: false, error: "temporary failure" } : schedules;

    expect(await cache.loadSchedules(load)).toEqual({ success: false, error: "temporary failure" });
    expect(await cache.loadSchedules(load)).toEqual(schedules);
    expect(calls).toBe(2);
  });

  test("invalidation notifies mounted cards and prevents old requests restoring stale data", async () => {
    const cache = createDiskCleanupScheduleCache();
    let calls = 0;
    let invalidations = 0;
    let resolveOld: ((result: BackendResponse<DiskCleanupSchedulesData>) => void) | undefined;
    const unsubscribe = cache.subscribeToScheduleInvalidation(() => { invalidations += 1; });
    const oldRead = cache.loadSchedules(() => {
      calls += 1;
      return new Promise<BackendResponse<DiskCleanupSchedulesData>>((resolve) => { resolveOld = resolve; });
    });
    await Promise.resolve();

    cache.invalidateSchedules();
    const freshRead = cache.loadSchedules(async () => {
      calls += 1;
      return { success: true, data: { schedules: [], total: 1 } };
    });
    resolveOld?.(schedules);

    await oldRead;
    const fresh = await freshRead;
    expect(invalidations).toBe(1);
    expect(calls).toBe(2);
    expect(fresh.data?.total).toBe(1);
    expect(await cache.loadSchedules(async () => schedules)).toEqual(fresh);
    unsubscribe();
  });
});
