import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ALL_CATEGORIES } from "../../panels/cleanup/cleanupCategories";
import { readCleanupScanResult } from "../../panels/cleanup/cleanupScanResult";
import { summarizeCleanupResult } from "../../panels/cleanup/cleanupDisplayModel";
import CleanupDisplayDetails from "./CleanupDisplayDetails";
import CleanupTraceCard from "./CleanupTraceCard";

const find = (id: string) => ALL_CATEGORIES.find(item => item.id === id)!;
const card = (id: string, data: Record<string, unknown>) => {
  const category = find(id);
  const result = readCleanupScanResult(category, { success: true, data });
  return { result, html: renderToStaticMarkup(<CleanupTraceCard category={category} count={result.count} preview={result.items} rawData={result.raw} error={result.error} loading={false} clearing={false} onViewDetails={() => {}} />) };
};

describe("clean, empty and not-applicable cleanup scans", () => {
  test("a PC without a cellular adapter is neutral and not applicable", () => {
    const raw = { files: [], total: 0, totalSizeMB: 0, interfaceCount: 0, noHardware: true, mobileService: "stopped", partial: false, status: "not_applicable", coverageNote: "Windows reports no mobile broadband adapter.", message: "No mobile broadband adapter is currently reported." };
    const { result, html } = card("cellularHistory", raw);
    expect(result.count).toBe(0);
    expect(result.error).toBeUndefined();
    expect(html).toContain('data-cleanup-state="not-applicable"');
    expect(html).toContain("Not applicable");
    for (const bad of ["Scan failed", "Scan incomplete", "Limited scan", "No mobile broadband adapter detected", "install its driver"]) expect(html).not.toContain(bad);
    const summary = summarizeCleanupResult({ id: "cellularHistory", count: 0, raw: result.raw });
    expect(summary.state).toBe("empty");
    expect(summary.message).toContain("Not applicable");
    const details = renderToStaticMarkup(<CleanupDisplayDetails category={find("cellularHistory")} mode="simple" count={0} items={result.items} rawData={result.raw} clearing={false} clearLabel="Clear" onClose={() => {}} />);
    expect(details).not.toContain("Scan failed");
    expect(details).not.toContain("Scan incomplete");
  });

  test("a real cellular adapter failure is still reported", () => {
    const { html } = card("cellularHistory", { files: [], total: 0, partial: true, status: "partial", message: "No readable cellular interface was reported; saved profiles could not be verified." });
    expect(html).toContain("Scan incomplete");
  });

  test("an empty Recycle Bin and verified WSL absence report as complete", () => {
    const bin = card("recycleBin", { items: [], total: 0, totalSizeKB: 0, unreadableRecords: 0, inaccessibleLocations: 0, partial: false, status: "complete", message: null });
    expect(bin.html).toContain('data-cleanup-state="clean"');
    expect(bin.html).not.toContain("Scan incomplete");
    const wsl = card("wslData", { files: [], total: 0, totalSizeMB: 0, partial: false, status: "complete", coverage: null, coverageNote: null, message: null });
    expect(wsl.html).toContain('data-cleanup-state="clean"');
    expect(wsl.html).not.toContain("Scan incomplete");
  });

  test("other accounts' Recycle Bins that cannot be opened are a scope note, not an incomplete scan", () => {
    const { html, result } = card("recycleBin", { items: [], total: 0, totalSizeKB: 0, unreadableRecords: 0, inaccessibleLocations: 0, otherAccountLocations: 2, coverage: "limited", coverageNote: "Recycle Bin records for 2 other-account locations need administrator access and were not checked.", partial: false, status: "complete", message: null });
    expect(html).toContain('data-cleanup-state="empty"');
    expect(html).not.toContain("Scan incomplete");
    expect(summarizeCleanupResult({ id: "recycleBin", count: 0, raw: result.raw }).message).toBe("No deleted items found");
  });

  test("unreadable Recycle Bin records still report an incomplete scan", () => {
    const { html } = card("recycleBin", { items: [], total: 0, unreadableRecords: 1, inaccessibleLocations: 1, partial: true, status: "partial", message: "Read 0 records; 1 records and 1 locations could not be read." });
    expect(html).toContain("Scan incomplete");
  });
});
