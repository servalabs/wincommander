import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { PANEL_MANIFESTS } from "../types/panels";
import { uiAuditBackendResponse, uiAuditDirectResponse } from "./uiAudit";
import { buildTraceView } from "../components/shared/traceTable";
import { shouldSkipStartupSplash } from "../lib/startupMode";
import { ALL_CATEGORIES } from "../panels/cleanup/cleanupCategories";
import { readCleanupScanResult } from "../panels/cleanup/cleanupScanResult";

describe("UI audit fixture", () => {
  test("skips only the dev audit splash so exhaustive route checks do not time out", () => {
    expect(shouldSkipStartupSplash(true, "/ui-audit.html")).toBe(true);
    expect(shouldSkipStartupSplash(true, "/nested/ui-audit.html")).toBe(true);
    expect(shouldSkipStartupSplash(true, "/")).toBe(false);
    expect(shouldSkipStartupSplash(false, "/ui-audit.html")).toBe(false);
  });

  test("loads every manifest-backed panel chunk through its production route", async () => {
    const ids = new Set<string>();
    for (const panel of PANEL_MANIFESTS) {
      expect(ids.has(panel.id)).toBe(false);
      ids.add(panel.id);
      const module = await panel.importFn();
      expect(typeof module.default).toBe("function");
    }
    expect(ids.size).toBe(PANEL_MANIFESTS.length);
  }, 15_000);

  test("exercises multi-column forensic datasets instead of one-column placeholders", () => {
    const view = buildTraceView(uiAuditDirectResponse("audit_trace_fixture"), []);
    const paths = view.datasets.find((dataset) => dataset.id === "paths");

    expect(view.datasets.length).toBeGreaterThanOrEqual(10);
    expect(view.datasets.every((dataset) => dataset.columns.length >= 2)).toBe(true);
    expect(paths?.columns).toContain("Path");
    expect(paths?.columns).toContain("Hive");
    expect(paths?.columns).toContain("Timestamp");
  });

  test("keeps cleanup viewer fixtures category-specific", () => {
    const dns = buildTraceView(uiAuditBackendResponse("Get-DnsCacheEntries"), []);
    const srum = buildTraceView(uiAuditBackendResponse("Get-SRUMData"), []);
    const cacheFiles = buildTraceView(uiAuditBackendResponse("Get-BranchCacheInfo"), []);
    const eventLogs = buildTraceView(uiAuditBackendResponse("Get-EventLogSummary"), []);

    expect(dns.datasets.map((dataset) => dataset.id)).toEqual(["entries"]);
    expect(srum.datasets.map((dataset) => dataset.id)).toEqual(["files"]);
    expect(cacheFiles.datasets.map((dataset) => dataset.id)).toEqual(["files"]);
    expect(eventLogs.datasets.map((dataset) => dataset.id)).toEqual(["logs"]);
    expect([dns, srum, cacheFiles, eventLogs].every((view) =>
      view.datasets.every((dataset) => dataset.columns.length >= 2),
    )).toBe(true);
  });

  test("gives every scannable cleanup category a focused rendered fixture", () => {
    const backendUrls = [
      new URL("../hooks/useBackend.ts", import.meta.url),
      new URL("../lib/searchMaintenanceClient.ts", import.meta.url),
    ];
    const backendSource = backendUrls
      .map((url) => readFileSync(decodeURIComponent(url.pathname).replace(/^\/([A-Za-z]:)/, "$1"), "utf8"))
      .join("\n");
    for (const category of ALL_CATEGORIES.filter((entry) => entry.getDataKey)) {
      const mappingIndex = backendSource.indexOf(`${category.getDataKey}:`);
      const command = mappingIndex >= 0
        ? backendSource.slice(mappingIndex).split(/\r?\n/).slice(0, 2).join("\n").match(/"(Get-[^"]+)"/)?.[1]
        : undefined;
      expect(command).toBeTruthy();

      const fixture = uiAuditBackendResponse(command!);
      const view = buildTraceView(fixture, []);
      expect(view.datasets.length).toBeGreaterThan(0);
      expect(view.datasets.length <= 2).toBe(true);
      expect(view.datasets.every((dataset) => dataset.columns.length >= 2)).toBe(true);
      const card = readCleanupScanResult(category, { success: true, data: fixture });
      if (card.error) throw new Error(`${category.id}: ${card.error}`);
      expect(card.error).toBeUndefined();
      expect(card.count > 0).toBe(true);
    }
  });
});
