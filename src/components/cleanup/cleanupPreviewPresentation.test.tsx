import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ALL_CATEGORIES } from "../../panels/cleanup/cleanupCategories";
import { isClipboardHistoryOff, summarizeCleanupResult } from "../../panels/cleanup/cleanupDisplayModel";
import CleanupRecordPreview, { getSafeCleanupPreview } from "./CleanupRecordPreview";
import CleanupTraceCard from "./CleanupTraceCard";
import { cleanupCardPreview, cleanupPreviewColumns } from "./cleanupPreviewPresentation";

test("preview drops only columns empty across the complete returned dataset", () => {
  const rows = Array.from({ length: 12 }, (_, index) => ({ label: `Record ${index + 1}`, facts: [
    `Name: item-${index}`, "Type: (empty)", "Missing: No value", "Whitespace:   ", "Count: 0", "Enabled: No",
    ...(index === 11 ? ["Later value: available"] : []),
  ] }));
  expect(cleanupPreviewColumns(rows)).toEqual(["Name", "Count", "Enabled", "Later value"]);
});

test("record navigation is above the table and preserves a column populated after the first page", () => {
  const raw = { entries: Array.from({ length: 12 }, (_, index) => ({ name: `item-${index}`, type: "", ttl: 0, ...(index === 11 ? { source: "Late-page source" } : {}) })) };
  const html = renderToStaticMarkup(<CleanupRecordPreview categoryId="dnsCache" rawData={raw} mode="advanced" />);
  expect(html).not.toContain('<th scope="col">Type</th>');
  expect(html).toContain('<th scope="col">Source</th>');
  expect(html).toContain('<th scope="col">TTL</th>');
  expect(html.indexOf('aria-label="Detail pages"') < html.indexOf("<table>")).toBe(true);
  expect(html).toContain("1–10 of 12 items");
  expect(html.match(/aria-label="Detail pages"/g)?.length).toBe(1);
});

test("outer card previews show useful values without generic record labels or raw field names", () => {
  const rows = getSafeCleanupPreview("servicingLogs", { files: [{ name: "CBS.log", path: "C:\\Logs\\CBS.log", sizeKB: 1 }] });
  expect(cleanupCardPreview(rows)).toEqual(["CBS.log · 1.0 KB"]);
  const category = ALL_CATEGORIES.find(item => item.id === "servicingLogs")!;
  const html = renderToStaticMarkup(<CleanupTraceCard category={category} count={1} preview={[]} rawData={{ files: [{ name: "CBS.log", path: "C:\\Logs\\CBS.log" }] }} loading={false} clearing={false} />);
  expect(html).toContain("CBS.log");
  expect(html).not.toContain("Record 1");
  expect(html).not.toContain("Name:");
  expect(html).not.toContain("Location:");
  expect(html).not.toContain("Regenerates after clear");
});

test("zero items in a limited scope use compact rows while retaining limited coverage", () => {
  const category = ALL_CATEGORIES.find(item => item.id === "developerCaches")!;
  const html = renderToStaticMarkup(<CleanupTraceCard category={category} compact count={0} preview={[]} rawData={{ files: [], total: 0, coverage: "limited" }} loading={false} clearing={false} onViewDetails={() => {}} />);
  expect(html).toContain("cleanup-trace-card--compact");
  expect(html).toContain('data-cleanup-state="limited"');
  expect(html).toContain("checked locations");
  expect(html).not.toContain('data-cleanup-state="clean"');
});

test("disabled clipboard history is explicit and never claims an empty or completed scan", () => {
  const category = ALL_CATEGORIES.find(item => item.id === "clipboardHistory")!;
  const raw = { historyItems: [], clipboardHistoryDisabled: true, historyReadStatus: "disabled", partial: true, status: "partial" };
  const summary = summarizeCleanupResult({ id: category.id, count: -1, raw, error: "History unavailable while disabled" });
  expect(summary.state).toBe("disabled");
  expect(summary.count).toBeNull();
  const html = renderToStaticMarkup(<CleanupTraceCard category={category} compact count={-1} preview={[]} rawData={raw} error="History unavailable while disabled" loading={false} clearing={false} onClear={() => {}} onViewDetails={() => {}} />);
  expect(html).toContain('data-cleanup-state="disabled"');
  expect(html).toContain("Clipboard history is off");
  expect(html).not.toContain("Scan incomplete");
  expect(html).not.toContain("No items found");
  expect(html).not.toContain(">Clear<");
  expect(isClipboardHistoryOff(category.id, { ...raw, error: true })).toBe(false);
  expect(summarizeCleanupResult({ id: category.id, count: -1, raw: { ...raw, error: true } }).state).toBe("error");
});
