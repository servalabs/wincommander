import { executeBackendCommand, type BackendResponse, type InstalledBrowser } from "../hooks/useBackend";

export interface BrowserInventory {
  browsers: InstalledBrowser[];
  count: number;
}

export function createBrowserInventoryCache<T>(
  load: () => Promise<T>,
  shouldCache: (value: T) => boolean = () => true,
) {
  let cachedValue: T | undefined;
  let hasCachedValue = false;
  let inFlight: Promise<T> | undefined;
  let generation = 0;

  const request = (refresh: boolean): Promise<T> => {
    if (!refresh && hasCachedValue) return Promise.resolve(cachedValue as T);
    if (!refresh && inFlight) return inFlight;
    if (refresh) {
      cachedValue = undefined;
      hasCachedValue = false;
    }

    const requestGeneration = ++generation;
    const pending = Promise.resolve()
      .then(load)
      .then((value) => {
        if (requestGeneration === generation && shouldCache(value)) {
          cachedValue = value;
          hasCachedValue = true;
        }
        return value;
      })
      .finally(() => {
        if (inFlight === pending) inFlight = undefined;
      });
    inFlight = pending;
    return pending;
  };

  return {
    get: () => request(false),
    refresh: () => request(true),
    peek: () => hasCachedValue ? cachedValue : undefined,
  };
}

export function createOnboardingExperiencePreloader(
  loadBrowserInventory: () => Promise<unknown>,
  loadScrubUi: () => Promise<unknown>,
): () => Promise<void> {
  let preloadPromise: Promise<void> | undefined;
  return () => {
    if (!preloadPromise) {
      preloadPromise = Promise.allSettled([loadBrowserInventory(), loadScrubUi()]).then(() => undefined);
    }
    return preloadPromise;
  };
}

const browserInventoryCache = createBrowserInventoryCache<BackendResponse<BrowserInventory>>(
  () => executeBackendCommand<BrowserInventory>("Get-InstalledBrowsersJson"),
  (result) => result.success,
);

export const getBrowserInventory = browserInventoryCache.get;
export const refreshBrowserInventory = browserInventoryCache.refresh;
export const getCachedBrowserInventory = browserInventoryCache.peek;

export const preloadOnboardingExperience = createOnboardingExperiencePreloader(
  getBrowserInventory,
  () => import("../components/MetadataScrubberDialog"),
);
