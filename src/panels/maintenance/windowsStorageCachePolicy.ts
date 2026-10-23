import type { CleanupCategory } from "../cleanup/cleanupCategories";

// Persistent app state, network records and location metadata stay in System Cleanup.
export const WINDOWS_STORAGE_CACHE_IDS = ["spotlightCache", "legacyIconCache", "branchCache"] as const;

export function getWindowsStorageCacheCategories(categories: readonly CleanupCategory[]): CleanupCategory[] {
  const byId = new Map(categories.map((category) => [category.id, category]));
  return WINDOWS_STORAGE_CACHE_IDS.flatMap((id) => {
    const category = byId.get(id);
    return category ? [category] : [];
  });
}
