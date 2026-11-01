import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { VIEW_ONLY_CATEGORIES } from "../../panels/cleanup/cleanupCategories";
import { observedProcessRows, summarizeCleanupResult } from "../../panels/cleanup/cleanupDisplayModel";
import { readCleanupScanResult } from "../../panels/cleanup/cleanupScanResult";
import CleanupTraceCard from "./CleanupTraceCard";
import CleanupDisplayDetails from "./CleanupDisplayDetails";

test("a partial process scan reports observed rows without claiming a complete count", () => {
  const category = VIEW_ONLY_CATEGORIES.find(item => item.id === "processIntel")!;
  const raw = { partial: true, status: "partial", processes: [
    { pid: 42, name: "one", path: null, pathStatus: "unavailable" },
    { pid: 43, name: "two", path: "C:\\two.exe" },
  ] };
  const result = readCleanupScanResult(category, { success: true, data: raw });
  expect(result.count).toBe(-1);
  expect(observedProcessRows(category.id, raw)).toBe(2);
  expect(summarizeCleanupResult({ id: category.id, count: result.count, raw: result.raw, error: result.error }).message).toBe("2 processes observed");
  const card = renderToStaticMarkup(<CleanupTraceCard category={category} count={result.count} preview={[]} loading={false} clearing={false} rawData={result.raw} error={result.error} onViewDetails={() => {}} />);
  expect(card).toContain("2 processes observed");
  expect(card).toContain("Scan incomplete");
  const details = renderToStaticMarkup(<CleanupDisplayDetails category={category} mode="advanced" count={result.count} items={[]} rawData={result.raw} error={result.error} clearing={false} clearLabel="Clear" onClose={() => {}} />);
  expect(details).toContain("2 processes observed");
  expect(details).toContain("one");
  expect(details).toContain("two");
  expect(details).toContain("unavailable");
});

test("malformed or failed process data never produces an observed count", () => {
  expect(observedProcessRows("processIntel", { status: "partial", partial: true, processes: [{ pid: 1, name: "a" }, { pid: 1, name: "b" }] })).toBeNull();
  expect(observedProcessRows("processIntel", { status: "partial", partial: true, processes: [{ name: "a" }] })).toBeNull();
  expect(observedProcessRows("processIntel", { status: "partial", partial: true, error: true, processes: [] })).toBeNull();
  expect(observedProcessRows("dnsCache", { status: "partial", partial: true, processes: [] })).toBeNull();
});
