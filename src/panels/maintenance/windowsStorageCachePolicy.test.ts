import { describe, expect, test } from "bun:test";
import { DEEP_DFIR_CATEGORIES, STANDARD_CATEGORIES } from "../cleanup/cleanupCategories";
import { getWindowsStorageCacheCategories, WINDOWS_STORAGE_CACHE_IDS } from "./windowsStorageCachePolicy";

const categories = [...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES];

describe("additional Windows storage caches", () => {
  test("keeps only the three separately previewable Windows cache actions", () => {
    const selected = getWindowsStorageCacheCategories(categories);
    expect(selected.map((category) => category.id)).toEqual([...WINDOWS_STORAGE_CACHE_IDS]);
    expect(selected.map((category) => category.clearDataKey)).toEqual([
      "clearSpotlightCache", "clearLegacyIconCache", "clearBranchCache",
    ]);
    expect(selected.every((category) => !category.actionOnly && !!category.extractPreview)).toBe(true);
  });

  test("does not pull privacy records, persistent app state or already-covered caches into storage", () => {
    const excluded = ["dnsCache", "geolocationCache", "photosCache", "xboxCache", "thumbnailDb", "fontCache", "p2pUpdateCache"];
    expect(excluded.every((id) => categories.some((category) => category.id === id))).toBe(true);
    expect(getWindowsStorageCacheCategories(categories.filter((category) => excluded.includes(category.id)))).toEqual([]);
  });

  test("does not substitute a different action when an approved category is absent", () => {
    expect(getWindowsStorageCacheCategories(categories.filter((category) => category.id !== "branchCache")).map((category) => category.id)).toEqual(["spotlightCache", "legacyIconCache"]);
  });
});
