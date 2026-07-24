import { expect, test } from "bun:test";
import type { RoutineCleanerItem, RoutineCleanerScan } from "../../hooks/useBackend";
import { getRecommendedItemIds, getRoutineCleanerCategories, getScanAfterClean, getSelectedItems, groupRoutineCleanerItems, WINDOWS_DISK_CLEANUP_CATEGORIES } from "./routineCleanerHelpers";

const items: RoutineCleanerItem[] = [
  { id: "system-1", category: "system", label: "Temp", path: "C:\\Temp", bytes: 200, fileCount: 2, recommended: true, operation: "delete", truncated: false },
  { id: "browser-1", category: "browsers", label: "Cache", path: "C:\\Cache", bytes: 100, fileCount: 1, recommended: false, operation: "delete", truncated: false },
  { id: "system-2", category: "system", label: "Icons", path: "C:\\Icons", bytes: 50, fileCount: 1, recommended: true, operation: "delete", truncated: false },
];

test("groups preview items by their backend category", () => {
  const groups = groupRoutineCleanerItems(items);
  expect(groups.system?.map((item) => item.id)).toEqual(["system-1", "system-2"]);
  expect(groups.browsers?.map((item) => item.id)).toEqual(["browser-1"]);
});

test("recommended selection excludes opt-in targets", () => {
  expect(getRecommendedItemIds(items)).toEqual(["system-1", "system-2"]);
});

test("selected item helper ignores IDs absent from the latest scan", () => {
  expect(getSelectedItems(items, new Set(["system-1", "expired-id"])).map((item) => item.id)).toEqual(["system-1"]);
});

test("clean reconciliation retains failed items and recomputes preview totals", () => {
  const scan: RoutineCleanerScan = { items, totalBytes: 350, totalFiles: 4, skippedTargets: 0, cancelled: false };
  const remaining = getScanAfterClean(scan, ["system-1"]);
  expect(remaining.items.map((item) => item.id)).toEqual(["browser-1", "system-2"]);
  expect(remaining.totalBytes).toBe(150);
  expect(remaining.totalFiles).toBe(2);
});

test("Windows Disk Cleanup owns all local cache categories", () => {
  expect(WINDOWS_DISK_CLEANUP_CATEGORIES).toEqual(["system", "browsers", "applications", "gaming", "databases"]);
  expect(getRoutineCleanerCategories(WINDOWS_DISK_CLEANUP_CATEGORIES).map((category) => category.id)).toEqual(WINDOWS_DISK_CLEANUP_CATEGORIES);
});
