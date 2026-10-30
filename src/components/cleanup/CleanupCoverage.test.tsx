import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ALL_CATEGORIES } from "../../panels/cleanup/cleanupCategories";
import { readCleanupScanResult, readCleanupUserSummary } from "../../panels/cleanup/cleanupScanResult";
import { cleanupScanFailure } from "../../panels/cleanup/cleanupScanFailure";
import { summarizeCleanupGroups, summarizeCleanupResult } from "../../panels/cleanup/cleanupDisplayModel";
import CleanupDisplayDetails from "./CleanupDisplayDetails";
import CleanupTraceCard from "./CleanupTraceCard";

const category = ALL_CATEGORIES.find(item => item.id === "walFiles")!;
const limited = { files: [], total: 0, coverage: "limited" };

describe("cleanup coverage presentation", () => {
  test("combined details retain each account's specific incomplete explanation and records", () => {
    const wsl = ALL_CATEGORIES.find(item => item.id === "wslData")!;
    const groups = [
      { title: "Other user", username: "S-1-5-21-1-2-3-4", count: -1, items: [], error: "Disks and provider-managed data were not inspected.", rawData: { partial: true, records: [{ name: "Fixture distribution" }] } },
      { title: "Second user", username: "S-1-5-21-1-2-3-5", count: -1, items: [], error: "Registry access was denied.", rawData: { error: true, records: [{ name: "Captured registration" }] } },
    ];
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={wsl} mode="advanced" count={-1} items={[]} groupedItems={groups} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(html).toContain("Disks and provider-managed data were not inspected.");
    expect(html).toContain("Registry access was denied.");
    expect(html).toContain("Fixture distribution");
    expect(html).toContain("Captured registration");
  });
  test("an empty VM inventory with limited coverage is not shown as clean", () => {
    const vm = ALL_CATEGORIES.find(item => item.id === "virtualMachineArtifacts")!;
    const raw = { files: [], total: 0, coverage: "limited", coverageNote: "Relocated machines were not fully inspected." };
    const html = renderToStaticMarkup(<CleanupTraceCard category={vm} count={0} preview={[]} rawData={raw} loading={false} clearing={false} onViewDetails={() => {}} />);
    expect(html).toContain('data-cleanup-state="limited"');
    expect(html).not.toContain('data-cleanup-state="clean"');
  });
  test("an empty partial selected-account scan retains its coverage and stays incomplete", () => {
    const cache = ALL_CATEGORIES.find(item => item.id === "passwordManagerCaches")!;
    const result = readCleanupUserSummary({ count: 0, records: [], partial: true, status: "partial", coverage: "Selected locations only", message: "Only selected locations were checked." });
    expect(result.count).toBe(-1);
    const card = renderToStaticMarkup(<CleanupTraceCard category={cache} count={result.count} preview={[]} rawData={result.raw} error={result.error} loading={false} clearing={false} onViewDetails={() => {}} />);
    expect(card).toContain('data-cleanup-state="incomplete"');
    expect(card).not.toContain("Scan failed");
    const details = renderToStaticMarkup(<CleanupDisplayDetails category={cache} mode="advanced" count={result.count} items={[]} rawData={result.raw} error={result.error} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(details).toContain("Scan incomplete");
    expect(details).toContain("Only selected locations were checked.");
    expect(details).toContain("Selected locations only");
    expect(details).not.toContain("No items found");
  });
  test("shows the selected-account scan's specific coverage explanation", () => {
    for (const [id, message] of [
      ["wslData", "Disks and provider-managed data were not inspected."],
      ["dockerDesktopData", "Custom locations and other engines were not inspected."],
      ["credentialManager", "Credential entries and names were not inspected."],
    ]) {
      const card = ALL_CATEGORIES.find(item => item.id === id)!;
      const result = readCleanupUserSummary({ id, count: 0, records: [], partial: true, status: "partial", message });
      const html = renderToStaticMarkup(<CleanupDisplayDetails category={card} mode="advanced" count={result.count} items={[]} rawData={result.raw} error={result.error} clearing={false} clearLabel="Clear" onClose={() => {}} />);
      expect(html).toContain(message);
      expect(html).toContain("Scan incomplete");
    }
  });
  test("a zero in a limited scope is not presented as a clean device", () => {
    const html = renderToStaticMarkup(<CleanupTraceCard category={category} count={0} preview={[]} rawData={limited} loading={false} clearing={false} onClear={() => {}} onViewDetails={() => {}} />);
    expect(html).toContain('data-cleanup-state="limited"');
    expect(html).toContain("checked locations");
    expect(html).toContain("View details");
    expect(summarizeCleanupResult({ id: category.id, count: 0, raw: limited }).state).toBe("limited");
    expect(summarizeCleanupGroups(0, [{ count: 0, items: [], rawData: limited }]).state).toBe("limited");
    expect(summarizeCleanupResult({ id: "searchIndex", count: 0, raw: { files: [], total: 0, coverage: "Only selected Search files" } }).state).toBe("limited");
  });

  test("SRUM metadata-only coverage remains limited when no database file is found", () => {
    const srum = ALL_CATEGORIES.find(item => item.id === "srumData")!;
    const raw = { files: [], total: 0, historicalRecordsAvailable: false, coverage: "Database file metadata only. Historical usage records inside SRUDB.dat are not decoded." };
    expect(summarizeCleanupResult({ id: srum.id, count: 0, raw }).state).toBe("limited");
    const html = renderToStaticMarkup(<CleanupDisplayDetails category={srum} mode="advanced" count={0} items={[]} rawData={raw} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(html).toContain("historical usage records are not decoded");
    expect(html).toContain("Limited to selected locations");
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
