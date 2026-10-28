import { describe, expect, test } from "bun:test";
import { DEEP_DFIR_CATEGORIES, STANDARD_CATEGORIES, VIEW_ONLY_CATEGORIES } from "./cleanupCategories";
import { readCleanupScanResult, readCleanupUserSummary } from "./cleanupScanResult";

const amcache = DEEP_DFIR_CATEGORIES.find(category => category.id === "amcache")!;
const eventLogs = STANDARD_CATEGORIES.find(category => category.id === "eventLogs")!;

describe("cleanup scan results", () => {
  test("known-empty user details retain their explicitly returned empty dataset", () => {
    expect(readCleanupUserSummary({ count: 0, records: [], items: [] })).toEqual({
      count: 0, items: [], raw: { records: [] },
    });
  });

  test("unavailable or partial user coverage never becomes a clean empty dataset", () => {
    for (const state of [{ error: true }, { partial: true }, { complete: false }, { status: 'unknown' }, { status: 'partial' }]) {
      const result = readCleanupUserSummary({ count: 0, records: [], ...state });
      expect(result.count).toBe(-1);
      expect(result.raw).toBeUndefined();
      expect(!!result.error).toBe(true);
    }
    expect(readCleanupUserSummary({ count: 0 }).raw).toBeUndefined();
    expect(readCleanupUserSummary({ count: 15, records: [] }).count).toBe(-1);
  });

  test("every scannable card declares its expected details shape", () => {
    for (const category of [...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES, ...VIEW_ONLY_CATEGORIES]) {
      expect((category.requiredScanArrays?.length ?? 0) > 0).toBe(true);
    }
  });

  test("missing detail records cannot be shown as clean", () => {
    for (const category of [...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES, ...VIEW_ONLY_CATEGORIES]) {
      const result = readCleanupScanResult(category, { success: true, data: { total: 0 } });
      expect(result.count).toBe(-1);
    }
  });
  test("a successful empty scan is explicitly zero", () => {
    const result = readCleanupScanResult(amcache, {
      success: true, data: { entries: [{ category: "InventoryApplication", count: 0, sample: [] }], total: 0 },
    });
    expect(result.count).toBe(0);
    expect(result.error).toBeUndefined();
  });

  test("a reader error cannot appear as zero findings", () => {
    const result = readCleanupScanResult(amcache, {
      success: true, data: { error: true, message: "Access denied", entries: [], total: 0 },
    });
    expect(result.count).toBe(-1);
    expect(result.error).toBe("Access denied");
  });

  test("incomplete or missing totals cannot appear as zero findings", () => {
    expect(readCleanupScanResult(amcache, { success: true, data: { entries: [] } }).error).toContain("incomplete");
    expect(readCleanupScanResult(amcache, { success: true, data: { entries: [], total: 0, partial: true } }).error).toContain("incomplete");
  });

  test("App Launch Cache rejects a total of 15 with zero-item rows", () => {
    const result = readCleanupScanResult(amcache, {
      success: true, data: { entries: [
        { category: "InventoryApplication", count: 0, sample: [] },
        { category: "InventoryApplicationFile", count: 0, sample: [] },
      ], total: 15 },
    });
    expect(result.count).toBe(-1);
    expect(result.error).toContain("disagrees");
  });

  test("a consistent total retains full count despite bounded samples", () => {
    const result = readCleanupScanResult(amcache, {
      success: true, data: { entries: [
        { category: "InventoryApplication", count: 15, sample: [{ id: "one" }] },
      ], total: 15 },
    });
    expect(result.count).toBe(15);
    expect(result.error).toBeUndefined();
  });

  test("another card cannot report findings with empty details", () => {
    const result = readCleanupScanResult(eventLogs, {
      success: true, data: { total: 15, logs: [] },
    });
    expect(result.count).toBe(-1);
    expect(result.error).toContain("disagrees");
  });

  test("event record total agrees with log channel summaries", () => {
    const result = readCleanupScanResult(eventLogs, {
      success: true, data: { total: 15, countUnit: "event records", logs: [
        { name: "Application", count: 11 }, { name: "System", count: 4 },
      ] },
    });
    expect(result.count).toBe(15);
    expect(result.items.length).toBe(2);
  });
});
