import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ALL_CATEGORIES } from "../../panels/cleanup/cleanupCategories";
import { readCleanupScanResult } from "../../panels/cleanup/cleanupScanResult";
import { cleanupScanFailure } from "../../panels/cleanup/cleanupScanFailure";
import { summarizeCleanupGroups, summarizeCleanupResult } from "../../panels/cleanup/cleanupDisplayModel";
import CleanupDisplayDetails from "./CleanupDisplayDetails";
import CleanupTraceCard from "./CleanupTraceCard";

const category = ALL_CATEGORIES.find(item => item.id === "walFiles")!;
const limited = { files: [], total: 0, coverage: "limited" };

describe("cleanup coverage presentation", () => {
  test("a zero in a limited scope is not presented as a clean device", () => {
    const html = renderToStaticMarkup(<CleanupTraceCard category={category} count={0} preview={[]} rawData={limited} loading={false} clearing={false} onClear={() => {}} onViewDetails={() => {}} />);
    expect(html).toContain('data-cleanup-state="limited"');
    expect(html).toContain("checked locations");
    expect(html).toContain("View details");
    expect(summarizeCleanupResult({ id: category.id, count: 0, raw: limited }).state).toBe("limited");
    expect(summarizeCleanupGroups(0, [{ count: 0, items: [], rawData: limited }]).state).toBe("limited");
    expect(summarizeCleanupResult({ id: "searchIndex", count: 0, raw: { files: [], total: 0, coverage: "Only selected Search files" } }).state).toBe("limited");
  });

  test("a capped partial result stays incomplete in the card and both detail views", () => {
    const raw = { files: [], total: 100, coverage: "limited", partial: true, truncated: true };
    const error = "A per-location limit was reached.";
    const card = renderToStaticMarkup(<CleanupTraceCard category={category} count={-1} preview={[]} rawData={raw} error={error} loading={false} clearing={false} onClear={() => {}} onViewDetails={() => {}} />);
    expect(card).toContain('data-cleanup-state="incomplete"');
    expect(card).toContain("View details");
    for (const mode of ["simple", "advanced"] as const) {
      const detail = renderToStaticMarkup(<CleanupDisplayDetails category={category} mode={mode} count={-1} items={[]} rawData={raw} error={error} clearing={false} clearLabel="Clear" onClose={() => {}} onClear={() => {}} />);
      expect(detail).toContain("Scan incomplete");
      expect(detail).toContain("five folder levels");
      expect(detail).not.toContain("No items found");
      expect(detail).toContain("disabled");
    }
  });

  test("empty and failed cards expose details; captured previews remain visible without exposing errors", () => {
    const history = ALL_CATEGORIES.find(item => item.id === "psHistory")!;
    const empty = renderToStaticMarkup(<CleanupTraceCard category={history} count={0} preview={[]} rawData={{ entries: [], total: 0 }} loading={false} clearing={false} onClear={() => {}} onViewDetails={() => {}} />);
    expect(empty).toContain(`View details for ${history.label}`);
    const failed = renderToStaticMarkup(<CleanupTraceCard category={history} count={-1} preview={[]} error="C:\\Private\\secret-token" loading={false} clearing={false} onClear={() => {}} onViewDetails={() => {}} />);
    expect(failed).toContain(`View details for ${history.label}`);
    expect(failed).not.toContain("secret-token");
    const populated = renderToStaticMarkup(<CleanupTraceCard category={history} count={1} preview={["password=secret-token"]} rawData={{ entries: [{ command: "password=secret-token" }], total: 1 }} loading={false} clearing={false} onClear={() => {}} onViewDetails={() => {}} />);
    expect(populated).toContain("password=secret-token");
  });

  test("shows captured records even when their inventory total is inconsistent", () => {
    const raw = { files: [{ name: "observed.db", path: "C:\\Example\\observed.db" }], total: 2 };
    const result = readCleanupScanResult(category, { success: true, data: raw });
    expect(result.count).toBe(-1);
    expect(result.raw).toEqual(raw);
    const detail = renderToStaticMarkup(<CleanupDisplayDetails category={category} mode="advanced" count={result.count} items={result.items} rawData={result.raw} error={result.error} clearing={false} clearLabel="Clear" onClose={() => {}} onClear={() => {}} />);
    expect(detail).toContain("Scan failed");
    expect(detail).toContain("observed.db");
    expect(detail).toContain("disabled");
  });

  test("retains rows from a failed cleanup backend response without enabling Clear", () => {
    const raw = { error: true, message: "Source unavailable", files: [{ name: "observed.db", path: "C:\\Example\\observed.db" }] };
    const response = cleanupScanFailure("Get-TestScan", raw, "Source unavailable", true);
    const result = readCleanupScanResult(category, response);
    expect(result.count).toBe(-1);
    const detail = renderToStaticMarkup(<CleanupDisplayDetails category={category} mode="advanced" count={result.count} items={result.items} rawData={result.raw} error={result.error} clearing={false} clearLabel="Clear" onClose={() => {}} onClear={() => {}} />);
    expect(detail).toContain("observed.db");
    expect(detail).toContain("disabled");
    expect(cleanupScanFailure("Get-TestScan", raw, "failed", false).data).toBeUndefined();
    expect(cleanupScanFailure("Clear-Test", raw, "failed", true).data).toBeUndefined();
  });

  test("unknown preview counts stay unknown and account totals use record previews", () => {
    expect(summarizeCleanupResult({ id: "psHistory", count: -1 }).sampleCount).toBeNull();
    expect(summarizeCleanupGroups(3, [{ count: 3, items: ["one"], rawData: { records: [{}, {}] } }]).sampleCount).toBe(2);
    expect(summarizeCleanupGroups(3, [{ count: 3, items: ["one"] }]).sampleCount).toBeNull();
  });

  test("selected account activity records are recognized without a current-user sections field", () => {
    const history = ALL_CATEGORIES.find(item => item.id === "ntUserTraces")!;
    const flat = { records: [{ entry: "TypedPaths: C:\\Example" }], truncated: false };
    expect(summarizeCleanupResult({ id: history.id, count: 1, raw: flat }).state).toBe("found");
    const detail = renderToStaticMarkup(<CleanupDisplayDetails category={history} mode="advanced" count={1} items={[]} rawData={flat} accountLabel="Alice" clearing={false} clearLabel="Clear" onClose={() => {}} onClear={() => {}} />);
    expect(detail).toContain("C:\\Example");
    expect(detail).not.toContain("Scan details unavailable");
    const grouped = { records: [{ name: "TypedPaths", count: 1, entries: [{ value: "C:\\Example" }] }], truncated: false };
    expect(summarizeCleanupResult({ id: history.id, count: 1, raw: grouped }).state).toBe("found");
    expect(summarizeCleanupResult({ id: history.id, count: 2, raw: grouped }).state).toBe("inconsistent");
    expect(summarizeCleanupResult({ id: history.id, count: 0, raw: { records: [] } }).state).toBe("empty");
  });
});
