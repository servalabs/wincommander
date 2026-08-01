import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { PANEL_MANIFESTS } from "../types/panels";
import { createUiAuditSettings, uiAuditBackendResponse, uiAuditDirectResponse } from "./uiAudit";
import { buildTraceView } from "../components/shared/traceTable";

describe("UI audit fixture", () => {
  test("makes every manifest-backed panel reachable without persisted hiding", () => {
    const settings = createUiAuditSettings();
    expect(settings.app.permanentlyHiddenPanels).toEqual([]);
    expect(settings.app.lockedPanelIds).toEqual([]);

    for (const panel of PANEL_MANIFESTS) {
      expect(panel.id).toBeTruthy();
    }
  });

  test("enables every module used by a routed panel", () => {
    const modules = createUiAuditSettings().app.modules ?? {};
    expect(Object.values(modules).every(Boolean)).toBe(true);
  });

  test("keeps every structured trace dataset at its natural height", () => {
    const cssUrl = new URL("../components/shared/TraceDetailDialog.css", import.meta.url);
    const cssPath = decodeURIComponent(cssUrl.pathname).replace(/^\/([A-Za-z]:)/, "$1");
    const css = readFileSync(cssPath, "utf8");
    const itemsRule = css.match(/\.trace-dialog__items\s*\{([\s\S]*?)\}/)?.[1] ?? "";

    expect(itemsRule).toContain("grid-auto-rows: max-content");
    expect(itemsRule).toContain("align-content: start");
  });

  test("exercises multi-column forensic datasets instead of one-column placeholders", () => {
    const view = buildTraceView(uiAuditDirectResponse("audit_trace_fixture"), []);
    const paths = view.datasets.find((dataset) => dataset.id === "paths");

    expect(view.datasets.length).toBeGreaterThanOrEqual(10);
    expect(view.datasets.every((dataset) => dataset.columns.length >= 2)).toBe(true);
    expect(paths?.columns).toContain("Path");
    expect(paths?.columns).toContain("Hive");
    expect(paths?.columns).toContain("Timestamp");
  });

  test("returns array-backed fixtures for every mounted system manager", () => {
    for (const command of ["Get-StartupItems", "Get-LocalLoginUsers", "Get-AllScheduledTasks", "Get-AllServices"]) {
      expect(uiAuditBackendResponse(command)).toEqual([]);
    }

    expect((uiAuditDirectResponse("scan_runtimes") as { runtimes: unknown[] }).runtimes).toEqual([]);
    expect((uiAuditDirectResponse("runtime_visibility_state") as { state: { entries: unknown[] } }).state.entries).toEqual([]);
  });

  test("matches the array contracts consumed by driver and security views", () => {
    expect((uiAuditDirectResponse("startup_impact_scan") as { entries: unknown[] }).entries).toEqual([]);
    expect((uiAuditDirectResponse("driver_maintenance_inventory") as { drivers: unknown[] }).drivers).toEqual([]);
    expect((uiAuditDirectResponse("get_driver_health") as { devices: unknown[] }).devices).toEqual([]);
    expect((uiAuditDirectResponse("get_vulnerable_drivers") as { vulnerable: unknown[] }).vulnerable).toEqual([]);
    expect((uiAuditDirectResponse("malware_quarantine_list") as { entries: unknown[] }).entries).toEqual([]);
  });
});
