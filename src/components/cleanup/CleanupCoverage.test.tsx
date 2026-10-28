import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ALL_CATEGORIES } from "../../panels/cleanup/cleanupCategories";
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

  test("empty and failed cards still expose details without leaking preview or error text", () => {
    const history = ALL_CATEGORIES.find(item => item.id === "psHistory")!;
    const empty = renderToStaticMarkup(<CleanupTraceCard category={history} count={0} preview={[]} rawData={{ entries: [], total: 0 }} loading={false} clearing={false} onClear={() => {}} onViewDetails={() => {}} />);
    expect(empty).toContain(`View details for ${history.label}`);
    const failed = renderToStaticMarkup(<CleanupTraceCard category={history} count={-1} preview={[]} error="C:\\Private\\secret-token" loading={false} clearing={false} onClear={() => {}} onViewDetails={() => {}} />);
    expect(failed).toContain(`View details for ${history.label}`);
    expect(failed).not.toContain("secret-token");
    const populated = renderToStaticMarkup(<CleanupTraceCard category={history} count={1} preview={["password=secret-token"]} rawData={{ entries: [{ command: "password=secret-token" }], total: 1 }} loading={false} clearing={false} onClear={() => {}} onViewDetails={() => {}} />);
    expect(populated).not.toContain("secret-token");
  });

  test("unknown preview counts stay unknown and account totals use record previews", () => {
    expect(summarizeCleanupResult({ id: "psHistory", count: -1 }).sampleCount).toBeNull();
    expect(summarizeCleanupGroups(3, [{ count: 3, items: ["one"], rawData: { records: [{}, {}] } }]).sampleCount).toBe(2);
    expect(summarizeCleanupGroups(3, [{ count: 3, items: ["one"] }]).sampleCount).toBeNull();
  });
});
