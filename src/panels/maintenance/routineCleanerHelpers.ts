import type { RoutineCleanerCategory, RoutineCleanerItem, RoutineCleanerScan } from "../../hooks/useBackend";

export const ROUTINE_CLEANER_CATEGORIES: Array<{ id: RoutineCleanerCategory; label: string; description: string }> = [
  { id: "system", label: "System", description: "Temporary files, caches, and diagnostics" },
  { id: "browsers", label: "Browsers", description: "Regenerable browser caches" },
  { id: "applications", label: "Applications", description: "Application cache data" },
  { id: "gaming", label: "Gaming", description: "Launcher and shader caches" },
  { id: "databases", label: "Databases", description: "Safe SQLite optimization targets" },
];

/**
 * Categories the app-cache scope of "Reclaim disk space" owns. "system" is
 * deliberately excluded: every path Get-DiskCleanupScan offers is already a
 * clean target in maintenance-rules/win32/system.json, so exposing both put
 * two engines on the same nine paths. Windows-owned storage is the granular
 * cleanup scope's job; this scope only touches app-owned regenerable data.
 */
export const APP_CACHE_CLEANUP_CATEGORIES: RoutineCleanerCategory[] = [
  "browsers",
  "applications",
  "gaming",
  "databases",
];

export function getRoutineCleanerCategories(ids: RoutineCleanerCategory[]) {
  const permitted = new Set(ids);
  return ROUTINE_CLEANER_CATEGORIES.filter((category) => permitted.has(category.id));
}

export type RoutineCleanerGroups = Partial<Record<RoutineCleanerCategory, RoutineCleanerItem[]>>;

export function groupRoutineCleanerItems(items: RoutineCleanerItem[]): RoutineCleanerGroups {
  return items.reduce<RoutineCleanerGroups>((groups, item) => {
    const group = groups[item.category] ?? [];
    group.push(item);
    groups[item.category] = group;
    return groups;
  }, {});
}

export interface RoutineCleanerCategoryGroup {
  id: RoutineCleanerCategory;
  label: string;
  items: RoutineCleanerItem[];
}

/**
 * Categories that actually matched something in the latest scan, in the
 * canonical category order — drives the per-category results tabs. Categories
 * with zero matches are skipped entirely rather than rendered as an empty tab.
 */
export function getPopulatedRoutineCleanerCategories(items: RoutineCleanerItem[]): RoutineCleanerCategoryGroup[] {
  const groups = groupRoutineCleanerItems(items);
  return ROUTINE_CLEANER_CATEGORIES
    .map((category) => ({ id: category.id, label: category.label, items: groups[category.id] ?? [] }))
    .filter((category) => category.items.length > 0);
}

export function getRecommendedItemIds(items: RoutineCleanerItem[]): string[] {
  return items.filter((item) => item.recommended).map((item) => item.id);
}

export function getSelectedItems(items: RoutineCleanerItem[], selectedIds: Set<string>): RoutineCleanerItem[] {
  return items.filter((item) => selectedIds.has(item.id));
}

export function getScanAfterClean(scan: RoutineCleanerScan, cleanedIds: string[]): RoutineCleanerScan {
  const cleaned = new Set(cleanedIds);
  const items = scan.items.filter((item) => !cleaned.has(item.id));
  return {
    ...scan,
    items,
    totalBytes: items.reduce((total, item) => total + item.bytes, 0),
    totalFiles: items.reduce((total, item) => total + item.fileCount, 0),
  };
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length);
  return `${(bytes / 1024 ** exponent).toFixed(exponent > 1 ? 1 : 0)} ${units[exponent - 1]}`;
}
