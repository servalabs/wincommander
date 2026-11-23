import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { VIEW_ONLY_CATEGORIES } from "../../panels/cleanup/cleanupCategories";
import { observedProcessRows, summarizeCleanupResult } from "../../panels/cleanup/cleanupDisplayModel";
import { readCleanupScanResult } from "../../panels/cleanup/cleanupScanResult";
import CleanupTraceCard from "./CleanupTraceCard";
import CleanupDisplayDetails from "./CleanupDisplayDetails";

test("a partial process scan shows its rows with a warning note and no error", () => {
  const category = VIEW_ONLY_CATEGORIES.find(item => item.id === "processIntel")!;
  const raw = { partial: true, status: "partial", unavailableDetails: 1, skippedProcesses: 1, processes: [
    { pid: 42, name: "one", path: null, pathStatus: "unavailable" },
    { pid: 43, name: "two", path: "C:\\two.exe" },
  ] };
  const result = readCleanupScanResult(category, { success: true, data: raw });
  expect(result.error).toBeUndefined();
  expect(result.count).toBe(2);
  expect(observedProcessRows(category.id, raw)).toBe(2);
  const summary = summarizeCleanupResult({ id: category.id, count: result.count, raw: result.raw, error: result.error });
  expect(summary.state).toBe("found");
  expect(summary.nextStep).toContain("1 of 2 processes have limited details");
  expect(summary.nextStep).toContain("Windows protects some system processes");
  expect(summary.nextStep).toContain("1 process could not be listed");
  expect(summarizeCleanupResult({ id: category.id, count: result.count, raw: result.raw, error: result.error }).message).toBe("2 processes observed");
  const card = renderToStaticMarkup(<CleanupTraceCard category={category} count={result.count} preview={[]} loading={false} clearing={false} rawData={result.raw} error={result.error} onViewDetails={() => {}} />);
  expect(card).toContain("2 processes found");
  expect(card).toContain("1 of 2 processes have limited details");
  expect(card).not.toContain("Scan incomplete");
  expect(card).not.toContain("Scan failed");
  expect(card).not.toContain('role="alert"');
  const details = renderToStaticMarkup(<CleanupDisplayDetails category={category} mode="advanced" count={result.count} items={[]} rawData={result.raw} error={result.error} clearing={false} clearLabel="Clear" onClose={() => {}} />);
  expect(details).toContain("2 processes observed");
  expect(details).toContain("data-cleanup-process-warning");
  expect(details).toContain("1 of 2 processes have limited details");
  expect(details).toContain("one");
  expect(details).toContain("two");
  expect(details).toContain("unavailable");
});

test("a process scan that read no process at all stays a failure", () => {
  const category = VIEW_ONLY_CATEGORIES.find(item => item.id === "processIntel")!;
  const failed = readCleanupScanResult(category, { success: true, data: { error: true, message: "Failed to get process intelligence: No process could be read.", processes: [] } });
  expect(failed.count).toBe(-1);
  expect(failed.error).toContain("Failed to get process intelligence");
  const empty = readCleanupScanResult(category, { success: true, data: { partial: true, status: "partial", processes: [] } });
  expect(empty.count).toBe(-1);
  expect(empty.error).toBeTruthy();
});

test("malformed or failed process data never produces an observed count", () => {
  expect(observedProcessRows("processIntel", { status: "partial", partial: true, processes: [{ pid: 1, name: "a" }, { pid: 1, name: "b" }] })).toBeNull();
  expect(observedProcessRows("processIntel", { status: "partial", partial: true, processes: [{ name: "a" }] })).toBeNull();
  expect(observedProcessRows("processIntel", { status: "partial", partial: true, error: true, processes: [] })).toBeNull();
  expect(observedProcessRows("dnsCache", { status: "partial", partial: true, processes: [] })).toBeNull();
});
