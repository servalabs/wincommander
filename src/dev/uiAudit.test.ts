import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { PANEL_MANIFESTS } from "../types/panels";
import { createUiAuditSettings, UI_AUDIT_ARRAY_COMMANDS, UI_AUDIT_POPULATED_ARRAY_COMMANDS, uiAuditBackendResponse, uiAuditDirectResponse } from "./uiAudit";
import { buildTraceView } from "../components/shared/traceTable";
import { shouldSkipStartupSplash } from "../lib/startupMode";

describe("UI audit fixture", () => {
  test("skips only the dev audit splash so exhaustive route checks do not time out", () => {
    expect(shouldSkipStartupSplash(true, "/ui-audit.html")).toBe(true);
    expect(shouldSkipStartupSplash(true, "/nested/ui-audit.html")).toBe(true);
    expect(shouldSkipStartupSplash(true, "/")).toBe(false);
    expect(shouldSkipStartupSplash(false, "/ui-audit.html")).toBe(false);
  });

  test("makes every manifest-backed panel reachable without persisted hiding", () => {
    const settings = createUiAuditSettings();
    expect(settings.app.permanentlyHiddenPanels).toEqual([]);
    expect(settings.app.lockedPanelIds).toEqual([]);

    for (const panel of PANEL_MANIFESTS) {
      expect(panel.id).toBeTruthy();
    }
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

  test("populates every mounted system manager with contract-shaped rows", () => {
    for (const command of ["Get-StartupItems", "Get-LocalLoginUsers", "Get-AllScheduledTasks", "Get-AllServices"]) {
      const rows = uiAuditBackendResponse(command) as Record<string, unknown>[];
      expect(rows.length).toBeGreaterThan(0);
      expect(Object.keys(rows[0] ?? {}).length).toBeGreaterThanOrEqual(6);
    }

    const runtimes = (uiAuditDirectResponse("scan_runtimes") as { runtimes: Record<string, unknown>[] }).runtimes;
    expect(runtimes.length).toBeGreaterThan(0);
    expect(runtimes[0]).toMatchObject({ name: "audit-tray.exe", kind: "TypeB", hideable: true });
    expect((uiAuditDirectResponse("runtime_visibility_state") as { state: { entries: unknown[] } }).state.entries.length).toBeGreaterThan(0);
  });

  test("supplies the activation shape consumed by the Settings panel", () => {
    expect(uiAuditBackendResponse("Get-ActivationStatus")).toEqual({
      windows: { activated: true, edition: "Windows 11 Pro" },
      office: { installed: false },
    });
  });

  test("populates maintenance, driver, security, and network contracts", () => {
    for (const command of [
      "startup_impact_scan",
      "registry_cleaner_scan",
      "explorer_context_menu_scan",
      "environment_cleaner_scan",
      "uninstall_leftovers_scan",
      "malware_quarantine_list",
    ]) {
      const rows = (uiAuditDirectResponse(command) as { entries: Record<string, unknown>[] }).entries;
      expect(rows.length).toBeGreaterThan(0);
      expect(Object.keys(rows[0] ?? {}).length).toBeGreaterThanOrEqual(3);
    }

    expect((uiAuditDirectResponse("shortcut_cleaner_scan") as { shortcuts: unknown[] }).shortcuts.length).toBeGreaterThan(0);
    expect((uiAuditDirectResponse("driver_maintenance_inventory") as { drivers: unknown[] }).drivers.length).toBeGreaterThan(0);
    expect((uiAuditDirectResponse("get_driver_health") as { devices: unknown[] }).devices.length).toBeGreaterThan(0);
    expect((uiAuditDirectResponse("get_vulnerable_drivers") as { vulnerable: unknown[] }).vulnerable.length).toBeGreaterThan(0);

    const ports = uiAuditBackendResponse("Get-NetworkPorts") as { rows: unknown[]; totals: { shown: number } };
    expect(ports.rows.length).toBeGreaterThan(1);
    expect(ports.totals.shown).toBe(ports.rows.length);
    const adapters = uiAuditBackendResponse("Get-PhysicalNetworkAdapters") as { adapters: unknown[] };
    expect(adapters.adapters.length).toBeGreaterThan(1);

    expect(uiAuditDirectResponse("get_network_honeypot_bind_all_interfaces")).toBe(false);
    expect(uiAuditDirectResponse("network_honeypot_status")).toMatchObject({ running: false, armedPorts: [] });
    expect(uiAuditDirectResponse("get_ping_block_status")).toEqual({ blocked: false });
    expect(uiAuditDirectResponse("wifi_guard_status")).toMatchObject({ running: false, currentSsid: "Audit Wi-Fi" });
    const firewallBlocks = uiAuditBackendResponse("Get-ProtocolBlocks") as { blocks: unknown[] };
    expect(firewallBlocks.blocks.length).toBeGreaterThan(0);
  });

  test("populates install, update, and debloat app states", () => {
    const inventory = uiAuditBackendResponse("Get-AppInventory") as {
      manifestApps: Array<{ installed: boolean; updateAvailable: boolean }>;
      pendingUpdates: unknown[];
    };
    expect(inventory.manifestApps.some((app) => !app.installed)).toBe(true);
    expect(inventory.manifestApps.some((app) => app.installed && app.updateAvailable)).toBe(true);
    expect(inventory.pendingUpdates.length).toBeGreaterThan(0);

    const appx = uiAuditBackendResponse("Get-InstalledAppxInventory") as { apps: unknown[] };
    const bcu = uiAuditBackendResponse("Get-BcuApplicationList") as { apps: unknown[] };
    expect(appx.apps.length).toBeGreaterThan(0);
    expect(bcu.apps.length).toBeGreaterThan(0);
    expect(uiAuditBackendResponse("Test-BcuInstalled")).toMatchObject({ installed: true });
    expect(uiAuditBackendResponse("Test-EdgeInstalled")).toMatchObject({ installed: true });
    expect(uiAuditBackendResponse("Test-OneDriveInstalled")).toMatchObject({ installed: true });
    expect(uiAuditBackendResponse("Get-TeamsStatus")).toMatchObject({ installed: true });
  });

  test("populates the irreversible-storage review without enabling an erase", () => {
    const volumes = uiAuditBackendResponse("Get-BitLockerVolumes") as Array<{
      mountPoint: string;
      volumeType: string;
      protectorTypes: string[];
      recoveryPasswordPresent: boolean;
    }>;
    expect(volumes.length).toBeGreaterThan(1);
    expect(volumes.some((volume) => volume.volumeType === "OperatingSystem")).toBe(true);
    expect(volumes.some((volume) => volume.volumeType === "Data")).toBe(true);
    expect(volumes.every((volume) => volume.protectorTypes.length > 0)).toBe(true);
    expect(volumes.every((volume) => volume.recoveryPasswordPresent)).toBe(true);
  });

  test("populates the Dashboard public-network readout", () => {
    expect(uiAuditDirectResponse("get_public_ip_trace")).toEqual({ ip: "203.0.113.42" });
    expect(uiAuditBackendResponse("Get-DNSStatus")).toMatchObject({
      provider: "Cloudflare",
      servers: ["1.1.1.1", "1.0.0.1"],
    });
  });

  test("returns arrays for every typed array IPC contract used by the audit surface", () => {
    for (const command of UI_AUDIT_ARRAY_COMMANDS) {
      expect(Array.isArray(uiAuditDirectResponse(command))).toBe(true);
    }
  });

  test("populates security and activity viewers so layout audits exercise real rows", () => {
    for (const command of UI_AUDIT_POPULATED_ARRAY_COMMANDS) {
      expect((uiAuditDirectResponse(command) as unknown[]).length).toBeGreaterThan(0);
    }

    const usbTimeline = uiAuditDirectResponse("get_usb_timeline") as {
      records: Record<string, unknown>;
      sessions: unknown[];
    };
    expect(Object.keys(usbTimeline.records).length).toBeGreaterThan(0);
    expect(usbTimeline.sessions.length).toBeGreaterThan(0);
  });
});
