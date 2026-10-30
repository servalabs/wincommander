import { describe, expect, test } from "bun:test";
import { DEEP_DFIR_CATEGORIES, STANDARD_CATEGORIES, VIEW_ONLY_CATEGORIES } from "./cleanupCategories";
import { readCleanupScanResult, readCleanupUserSummary } from "./cleanupScanResult";
import { summarizeCleanupResult } from "./cleanupDisplayModel";

const amcache = DEEP_DFIR_CATEGORIES.find(category => category.id === "amcache")!;
const eventLogs = STANDARD_CATEGORIES.find(category => category.id === "eventLogs")!;
const journals = STANDARD_CATEGORIES.find(category => category.id === "ntfsJournals")!;
const userTraces = DEEP_DFIR_CATEGORIES.find(category => category.id === "ntUserTraces")!;
const notepad = DEEP_DFIR_CATEGORIES.find(category => category.id === "notepadState")!;
const history = STANDARD_CATEGORIES.find(category => category.id === "psHistory")!;
const developerCaches = DEEP_DFIR_CATEGORIES.find(category => category.id === "developerCaches")!;
const clipboard = STANDARD_CATEGORIES.find(category => category.id === "clipboardHistory")!;

describe("cleanup scan results", () => {
  test("Wi-Fi profiles show returned names and limited scope", () => {
    const wifi = STANDARD_CATEGORIES.find(category => category.id === "wlanProfiles")!;
    const empty = readCleanupScanResult(wifi, { success: true, data: { profiles: [], total: 0, coverage: "limited" } });
    expect(summarizeCleanupResult({ id: wifi.id, count: empty.count, raw: empty.raw }).state).toBe("limited");
    const populated = readCleanupScanResult(wifi, { success: true, data: { profiles: [{ name: "Cafe" }], total: 1, coverage: "limited" } });
    expect(populated.count).toBe(1);
    expect(populated.items).toEqual(["Cafe"]);
  });

  test("known-empty user details retain their explicitly returned empty dataset", () => {
    expect(readCleanupUserSummary({ count: 0, records: [], items: [] })).toEqual({
      count: 0, items: [], raw: { records: [] },
    });
  });

  test("user summary retains only the fixed limited-coverage enum", () => {
    expect(readCleanupUserSummary({ count: 0, status: 'scoped', coverage: 'limited', records: [], sampleCount: 0 })).toEqual({
      count: 0, items: [], raw: { records: [], coverage: 'limited', sampleCount: 0 },
    });
    const missingScoped = readCleanupUserSummary({ count: 0, status: 'scoped', coverage: 'limited' });
    expect(missingScoped.count).toBe(-1);
    expect(missingScoped.raw).toBeUndefined();
    expect(readCleanupUserSummary({ count: 0, coverage: 'private arbitrary text', records: [] }).raw).toEqual({ records: [] });
    const partial = readCleanupUserSummary({ count: 0, coverage: 'limited', records: [], partial: true, message: 'Incomplete.' });
    expect(partial.count).toBe(-1);
    expect(partial.raw).toBeUndefined();
    expect(partial.error).toBe('Incomplete.');
  });

  test("unavailable or partial user coverage never becomes a clean empty dataset", () => {
    for (const state of [{ error: true }, { partial: true }, { complete: false }, { status: 'unknown' }, { status: 'partial' }]) {
      const result = readCleanupUserSummary({ count: 0, records: [], ...state });
      expect(result.count).toBe(-1);
      expect(result.raw).toBeUndefined();
      expect(!!result.error).toBe(true);
    }
    expect(readCleanupUserSummary({ count: 0 }).count).toBe(-1);
    expect(readCleanupUserSummary({ count: 15 }).count).toBe(-1);
    expect(readCleanupUserSummary({ count: 0, records: 'invalid' }).count).toBe(-1);
    expect(readCleanupUserSummary({ count: 1, records: [null] }).count).toBe(-1);
    expect(readCleanupUserSummary({ count: 15, records: [], sampleCount: 0, previewLimit: 0 })).toEqual({
      count: 15, items: [], raw: { records: [], sampleCount: 0, previewLimit: 0 },
    });
    expect(readCleanupUserSummary({ count: 2, records: [{}, {}], sampleCount: 0 }).count).toBe(-1);
  });

  test("user summary previews do not contradict counts or confuse category rows with records", () => {
    expect(readCleanupUserSummary({ count: 2, records: [], items: [] }).count).toBe(-1);
    expect(readCleanupUserSummary({ count: 0, records: [{}], items: [] }).count).toBe(-1);
    expect(readCleanupUserSummary({ count: 2, records: [], items: [], sampleCount: 0, previewLimit: 0 }).count).toBe(2);
    expect(readCleanupUserSummary({ count: 2, records: [], items: [], sampleCount: 0, truncated: true }).count).toBe(2);
    expect(readCleanupUserSummary({ id: 'ntUserTraces', count: 0, records: [{ name: 'RunMRU', count: 0, entries: [] }], items: ['RunMRU: 0'] }).count).toBe(0);
    expect(readCleanupUserSummary({ id: 'ntUserTraces', count: 7, records: [{ name: 'RunMRU', count: 7 }], items: [] }).count).toBe(7);
    expect(readCleanupUserSummary({ id: 'ntUserTraces', count: 2, records: [{ name: 'RunMRU', count: 0 }], items: [] }).count).toBe(-1);
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
    expect(readCleanupScanResult(amcache, { success: true, data: { entries: [], total: 0 } }).error).toContain("incomplete");
    expect(readCleanupScanResult(amcache, { success: true, data: { entries: [], total: 0, partial: true } }).error).toContain("incomplete");
  });

  test("partial scans retain their coverage metadata while the count stays unavailable", () => {
    for (const [id, field] of [["walFiles", "files"], ["recallDb", "databases"]] as const) {
      const category = DEEP_DFIR_CATEGORIES.find(item => item.id === id)!;
      const data = { [field]: [{ name: "fixture" }], total: 100, partial: true, truncated: true, coverage: "limited", message: "A per-root limit was reached." };
      const result = readCleanupScanResult(category, { success: true, data });
      expect(result.count).toBe(-1);
      expect(result.error).toContain("limit was reached");
      expect(result.raw).toEqual(data);
      const failed = readCleanupScanResult(category, { success: true, data: { ...data, error: true, message: "Reader failed." } });
      expect(failed.error).toBe("Reader failed.");
      expect(failed.raw).toBeUndefined();
    }
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

  test("full counts can coexist with an explicitly bounded empty preview", () => {
    const result = readCleanupScanResult(history, { success: true, data: { total: 15, entries: [], sampleCount: 0, previewLimit: 0 } });
    expect(result.count).toBe(15);
    expect(result.items).toEqual([]);
    expect(result.error).toBeUndefined();
    const caches = readCleanupScanResult(developerCaches, { success: true, data: { total: 15, files: [], previewLimit: 0, truncated: true } });
    expect(caches.count).toBe(15);
    const logs = readCleanupScanResult(eventLogs, { success: true, data: { total: 15, logs: [], sampleCount: 0, previewLimit: 0 } });
    expect(logs.count).toBe(-1);
    const impossible = readCleanupScanResult(eventLogs, { success: true, data: { total: 15, logs: [{ count: 16 }], truncated: true } });
    expect(impossible.count).toBe(-1);
  });

  test("totals with different units do not contradict the card count", () => {
    const result = readCleanupScanResult(journals, { success: true, data: { total: 2, journals: [{ drive: "C:", present: true }, { drive: "D:", present: false }] } });
    expect(result.count).toBe(1);
    expect(result.error).toBeUndefined();
    expect(readCleanupScanResult(journals, { success: true, data: { journals: [{ drive: "C:" }] } }).count).toBe(-1);
  });

  test("user activity category counts must agree with their total", () => {
    const inconsistent = readCleanupScanResult(userTraces, { success: true, data: { total: 12, sections: [{ name: "RunMRU", count: 0, entries: [] }] } });
    expect(inconsistent.count).toBe(-1);
    expect(inconsistent.error).toContain("disagrees");
    const valid = readCleanupScanResult(userTraces, { success: true, data: { total: 2, sections: [{ name: "RunMRU", count: 2, entries: [{}, {}] }] } });
    expect(valid.count).toBe(2);
    expect(readCleanupScanResult(userTraces, { success: true, data: { total: 2, sections: [{ name: "RunMRU", count: 2, entries: [] }] } }).count).toBe(-1);
  });

  test("file inventories do not silently turn missing totals into zero", () => {
    const fileCategories = [...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES, ...VIEW_ONLY_CATEGORIES]
      .filter(category => category.requiredScanArrays?.includes("files"));
    expect(fileCategories.length).toBeGreaterThan(40);
    for (const category of fileCategories) {
      const metadata = category.id === "developerCaches" ? { previewLimit: 200, truncated: false } : {};
      expect(readCleanupScanResult(category, { success: true, data: { files: [], ...metadata } }).count).toBe(-1);
      expect(readCleanupScanResult(category, { success: true, data: { total: "0", files: [], ...metadata } }).count).toBe(-1);
      expect(readCleanupScanResult(category, { success: true, data: { total: 0, files: [], ...metadata } }).count).toBe(0);
    }
    expect(readCleanupScanResult(notepad, { success: true, data: { total: 0, files: [{ name: "fixture" }] } }).count).toBe(-1);
    expect(readCleanupScanResult(notepad, { success: true, data: { total: 15, files: [] } }).count).toBe(-1);
    expect(readCleanupScanResult(notepad, { success: true, data: { total: 1, files: ["malformed"] } }).count).toBe(-1);
  });

  test("clipboard counts actual history without exposing contents or confusing enabled settings with findings", () => {
    const read = (data: unknown) => readCleanupScanResult(clipboard, { success: true, data });
    const enabled = { clipboardHistoryDisabled: false, cloudClipboardDisabled: false };
    expect(read({ ...enabled, historyReadStatus: "complete", historyItems: [] }).count).toBe(0);
    expect(read({ ...enabled, clipboardHistoryDisabled: true, historyReadStatus: "disabled", historyItems: [] }).count).toBe(-1);
    expect(read({ ...enabled, clipboardHistoryDisabled: true, historyReadStatus: "complete", historyItems: [] }).count).toBe(0);
    expect(read({ ...enabled, clipboardHistoryDisabled: true, historyReadStatus: "unavailable", partial: true, historyItems: [] }).count).toBe(-1);
    const populated = read({ ...enabled, historyReadStatus: "complete", historyItems: [{ preview: "secret" }, { preview: "private" }] });
    expect(populated.count).toBe(2);
    expect(populated.items).toEqual(["Clipboard item", "Clipboard item"]);
    expect(read({ ...enabled, historyReadStatus: "unavailable", historyItems: [] }).count).toBe(-1);
    expect(read({ ...enabled, historyItems: [] }).count).toBe(-1);
  });

  test("full inventories require a numeric total even when their arrays are empty", () => {
    for (const [id, field] of [["crashDumps", "dumps"], ["recallDb", "databases"], ["recycleBin", "items"], ["connectivityHistory", "entries"]] as const) {
      const category = [...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES].find(item => item.id === id)!;
      expect(readCleanupScanResult(category, { success: true, data: { [field]: [] } }).count).toBe(-1);
      expect(readCleanupScanResult(category, { success: true, data: { [field]: [], total: 0 } }).count).toBe(0);
    }
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
