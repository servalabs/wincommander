import type { BackendResponse } from "../../hooks/useBackend";

export interface DiskCleanupWindowsProfile {
  name: string;
  displayName?: string;
  path: string;
  sid?: string;
  isCurrent?: boolean;
}

export interface DiskCleanupSchedule {
  categoryId: string;
  taskName: string;
  enabled: boolean;
  intervalMinutes: number;
  managedByAutoSet?: boolean;
  targetUser: string | null;
  ownerAccount?: string | null;
  lastRun: string | null;
  lastResult: number | null;
}

export interface DiskCleanupProfilesData {
  profiles: DiskCleanupWindowsProfile[];
  total: number;
  currentUser: string;
  currentSid?: string;
  isAdmin: boolean;
}

export interface DiskCleanupSchedulesData {
  schedules: DiskCleanupSchedule[];
  total: number;
}

export type DiskCleanupStatusLoader<T> = () => Promise<BackendResponse<T>>;

export interface DiskCleanupStatusLoaders {
  getProfiles: DiskCleanupStatusLoader<DiskCleanupProfilesData>;
  getSchedules: DiskCleanupStatusLoader<DiskCleanupSchedulesData>;
}

interface CachedLoader<T> {
  load(loader: DiskCleanupStatusLoader<T>, refresh?: boolean): Promise<BackendResponse<T>>;
  invalidate(): void;
}

function createCachedLoader<T>(): CachedLoader<T> {
  let cached: BackendResponse<T> | null = null;
  let inFlight: Promise<BackendResponse<T>> | null = null;
  let generation = 0;

  return {
    load(loader, refresh = false) {
      if (inFlight) return inFlight;
      if (!refresh && cached?.success && cached.data !== undefined) return Promise.resolve(cached);

      const requestGeneration = generation;
      let request: Promise<BackendResponse<T>>;
      request = Promise.resolve()
        .then(loader)
        .catch((cause): BackendResponse<T> => ({ success: false, error: String(cause) }))
        .then((result) => {
          if (requestGeneration === generation && result.success && result.data !== undefined) cached = result;
          return result;
        })
        .finally(() => {
          if (inFlight === request) inFlight = null;
        });

      inFlight = request;
      return request;
    },
    invalidate() {
      cached = null;
      generation += 1;
      // Detach any request started before the write. It may still resolve for
      // its original caller, but it cannot repopulate the cache or block a
      // fresh read.
      inFlight = null;
    },
  };
}

export interface DiskCleanupScheduleCache {
  loadProfiles(loader: DiskCleanupStatusLoader<DiskCleanupProfilesData>, refresh?: boolean): Promise<BackendResponse<DiskCleanupProfilesData>>;
  loadSchedules(loader: DiskCleanupStatusLoader<DiskCleanupSchedulesData>, refresh?: boolean): Promise<BackendResponse<DiskCleanupSchedulesData>>;
  invalidateSchedules(): void;
  subscribeToScheduleInvalidation(listener: () => void): () => void;
  preload(loaders: DiskCleanupStatusLoaders): Promise<void>;
}

/** Create a per-session cache that is also independently testable. */
export function createDiskCleanupScheduleCache(): DiskCleanupScheduleCache {
  const profiles = createCachedLoader<DiskCleanupProfilesData>();
  const schedules = createCachedLoader<DiskCleanupSchedulesData>();
  const listeners = new Set<() => void>();

  const invalidateSchedules = () => {
    schedules.invalidate();
    for (const listener of listeners) listener();
  };

  return {
    loadProfiles: profiles.load,
    loadSchedules: schedules.load,
    invalidateSchedules,
    subscribeToScheduleInvalidation(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    preload(loaders) {
      return Promise.all([
        profiles.load(loaders.getProfiles),
        schedules.load(loaders.getSchedules),
      ]).then(() => undefined);
    },
  };
}

const diskCleanupScheduleCache = createDiskCleanupScheduleCache();

export const loadDiskCleanupProfiles = diskCleanupScheduleCache.loadProfiles;
export const loadDiskCleanupSchedules = diskCleanupScheduleCache.loadSchedules;
export const invalidateDiskCleanupScheduleStatus = diskCleanupScheduleCache.invalidateSchedules;
export const subscribeToDiskCleanupScheduleInvalidation = diskCleanupScheduleCache.subscribeToScheduleInvalidation;

/**
 * Warm the same read-only status used by Maintenance → Storage & files.
 * Per-request caches make startup warm-up and an early card visit share both
 * completed results and any requests already in progress.
 */
export const preloadDiskCleanupScheduleStatus = diskCleanupScheduleCache.preload;
