import { describe, expect, test } from "bun:test";
import { DEEP_DFIR_CATEGORIES } from "./cleanupCategories";

function previewFor(id: string, data: unknown) {
  const category = DEEP_DFIR_CATEGORIES.find((candidate) => candidate.id === id);
  if (!category) throw new Error(`Missing cleanup category ${id}`);
  return category.extractPreview(data);
}

describe("typed cleanup preview adapters", () => {
  test("keeps file-backed category labels and backend totals", () => {
    expect(previewFor("notepadState", {
      total: 2,
      files: [{ name: "TabState.bin" }, { name: "Session.json" }],
    })).toEqual({ count: 2, items: ["TabState.bin", "Session.json"] });
  });

  test("keeps the legacy zero total while still showing available file labels", () => {
    expect(previewFor("dockerDesktopData", {
      files: [{ name: "docker-desktop.vhdx" }],
    })).toEqual({ count: 0, items: ["docker-desktop.vhdx"] });
  });

  test("uses entry and path fallbacks without treating malformed rows as labels", () => {
    expect(previewFor("webCache", {
      total: 3,
      entries: [{ path: "C:\\Users\\Ada\\WebCacheV01.dat" }, {}, "not a record"],
    })).toEqual({ count: 3, items: ["C:\\Users\\Ada\\WebCacheV01.dat", "—"] });
  });

  test("prefers an explicitly returned empty files list over a stale entries fallback", () => {
    expect(previewFor("appLaunchHistory", {
      total: 0,
      files: [],
      entries: [{ path: "C:\\stale-entry" }],
    })).toEqual({ count: 0, items: [] });
  });
});
