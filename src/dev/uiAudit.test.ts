import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { PANEL_MANIFESTS } from "../types/panels";
import { createUiAuditLicenseStatus, createUiAuditPendingPurchase, createUiAuditSettings, UI_AUDIT_ARRAY_COMMANDS, UI_AUDIT_POPULATED_ARRAY_COMMANDS, uiAuditBackendResponse, uiAuditDirectResponse } from "./uiAudit";
import { buildTraceView } from "../components/shared/traceTable";
import { shouldSkipStartupSplash } from "../lib/startupMode";
import { ALL_CATEGORIES } from "../panels/cleanup/cleanupCategories";

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

  test("keeps cleanup viewer fixtures category-specific", () => {
    const dns = buildTraceView(uiAuditBackendResponse("Get-DnsCacheEntries"), []);
    const srum = buildTraceView(uiAuditBackendResponse("Get-SRUMData"), []);
    const cacheFiles = buildTraceView(uiAuditBackendResponse("Get-BranchCacheInfo"), []);
    const eventLogs = buildTraceView(uiAuditBackendResponse("Get-EventLogSummary"), []);

    expect(dns.datasets.map((dataset) => dataset.id)).toEqual(["entries"]);
    expect(srum.datasets.map((dataset) => dataset.id)).toEqual(["entries"]);
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
        ? backendSource.slice(mappingIndex, mappingIndex + 320).match(/"(Get-[^"]+)"/)?.[1]
        : undefined;
      expect(command).toBeTruthy();

      const view = buildTraceView(uiAuditBackendResponse(command!), []);
      expect(view.datasets.length).toBeGreaterThan(0);
      expect(view.datasets.length <= 2).toBe(true);
      expect(view.datasets.every((dataset) => dataset.columns.length >= 2)).toBe(true);
    }
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

  test("populates cleanup account scope instead of leaving the profile selector loading", () => {
    const profiles = uiAuditBackendResponse("Get-UserProfiles") as {
      profiles: Array<{ name: string; displayName: string; isCurrent: boolean }>;
      currentUser: string;
      isAdmin: boolean;
    };

    expect(profiles.isAdmin).toBe(true);
    expect(profiles.currentUser).toBe("AuditAnalyst");
    expect(profiles.profiles).toHaveLength(2);
    expect(profiles.profiles.find((profile) => profile.isCurrent)?.displayName).toBe("Audit Analyst");
  });

  test("renders Fleet access and Vault permissions with contract-shaped fixtures", () => {
    const users = uiAuditBackendResponse("Get-FleetAccessUsers") as {
      users: Array<{ name: string; sid: string; isCurrent: boolean }>;
    };
    const policy = uiAuditDirectResponse("get_vault_access_policy") as {
      entries: Array<{ label: string; grants: unknown[] }>;
    };
    const authorized = uiAuditDirectResponse("vault_list_authorized_entries") as Array<{ entry_id: string }>;

    expect(users.users).toHaveLength(2);
    expect(users.users.find((user) => user.isCurrent)?.sid).toBe("S-1-5-21-1000-1000-1000-1001");
    expect(uiAuditDirectResponse("get_vault_access_capabilities")).toEqual({ can_manage_policy: true });
    expect(policy.entries[0]).toMatchObject({ label: "Team documents" });
    expect(policy.entries[0]?.grants).toHaveLength(2);
    expect(authorized).toEqual([{ entry_id: "audit-team-vault", label: "Team documents", access: "write", presentation: "machine", container_kind: "standard", mount_state: "unmounted", drive_letter: null }]);
  });

  test("supplies the activation shape consumed by the Settings panel", () => {
    expect(uiAuditBackendResponse("Get-ActivationStatus")).toEqual({
      windows: { activated: true, edition: "Windows 11 Pro" },
      office: { installed: false },
    });
  });

  test("supports active and unlicensed rendered purchase states", () => {
    expect(createUiAuditLicenseStatus()).toMatchObject({ licensed: true, valid: true, plan: "Pro" });
    expect(createUiAuditLicenseStatus("unlicensed")).toMatchObject({
      configured: true,
      licensed: false,
      valid: false,
      features: [],
      trial_available: true,
    });
  });

  test("populates maintenance, driver, security, and network contracts", () => {
    const diskCleanup = uiAuditBackendResponse("Get-DiskCleanupScan") as {
      categories: Array<{ Id: string; FileCount: number; SizeMb: number }>;
    };
    expect(diskCleanup.categories.length).toBeGreaterThan(4);
    expect(diskCleanup.categories.find((category) => category.Id === "windowsOld")).toMatchObject({
      FileCount: 2_418,
      SizeMb: 13_107.2,
    });
    const routineCleaner = uiAuditDirectResponse("routine_cleaner_scan") as {
      items: Array<{ category: string; path: string; bytes: number; fileCount: number }>;
      totalBytes: number;
      totalFiles: number;
    };
    expect(routineCleaner.items.length).toBeGreaterThan(4);
    expect(new Set(routineCleaner.items.map((item) => item.category))).toEqual(
      new Set(["browsers", "applications", "gaming", "databases"]),
    );
    expect(routineCleaner.items.every((item) => item.path && item.bytes > 0 && item.fileCount > 0)).toBe(true);
    expect(routineCleaner.totalBytes).toBe(routineCleaner.items.reduce((sum, item) => sum + item.bytes, 0));
    expect(routineCleaner.totalFiles).toBe(routineCleaner.items.reduce((sum, item) => sum + item.fileCount, 0));
    const diskScan = uiAuditDirectResponse("run_disk_scan") as { scanRoot: string; totalSize: number; fileCount: number; folderCount: number };
    expect(diskScan).toMatchObject({ scanRoot: "C:\\Audit", fileCount: 8_421, folderCount: 614 });
    expect(diskScan.totalSize).toBeGreaterThan(10 * 1024 ** 3);
    const diskChildren = uiAuditDirectResponse("get_disk_children") as Array<{ fullPath: string; size: number; isDir: boolean }>;
    const largeDiskItems = uiAuditDirectResponse("get_large_disk_items") as Array<{ fullPath: string; itemType: string; cleanupHint: string; risk: string }>;
    expect(diskChildren.length).toBeGreaterThan(3);
    expect(new Set(diskChildren.map((item) => item.isDir))).toEqual(new Set([true, false]));
    expect(largeDiskItems.length).toBeGreaterThan(3);
    expect(largeDiskItems.every((item) => item.fullPath && item.itemType && item.cleanupHint && item.risk)).toBe(true);
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

  test("populates deep storage, mesh, flow, fleet, license, and update states", () => {
    const storage = uiAuditBackendResponse("Get-EncryptionStatus") as { volumes: unknown[] };
    const partitions = uiAuditBackendResponse("Get-EncryptionPartitions") as { partitions: unknown[] };
    const ramDisks = uiAuditBackendResponse("Get-RamDiskStatus") as { installed: boolean; disks: unknown[] };
    const mesh = uiAuditBackendResponse("Get-MeshVPNStatus") as { peers: Array<{ Hostname: string; ExitNodeOption: boolean }> };
    const rules = uiAuditDirectResponse("flow_list_rules") as Array<{ enabled: boolean; locked: boolean }>;

    expect(storage.volumes.length).toBeGreaterThan(1);
    expect(partitions.partitions.length).toBeGreaterThan(1);
    expect(uiAuditBackendResponse("Test-RamDiskInstalled")).toEqual({ installed: true });
    expect(ramDisks.installed).toBe(true);
    expect(ramDisks.disks.length).toBeGreaterThan(0);
    expect(uiAuditBackendResponse("Get-SystemRamInfo")).toMatchObject({ totalMB: 32768, freeMB: 20480 });
    expect(mesh.peers.every((peer) => peer.Hostname.length > 0)).toBe(true);
    expect(mesh.peers.some((peer) => peer.ExitNodeOption)).toBe(true);
    expect(rules.some((rule) => rule.enabled && !rule.locked)).toBe(true);
    expect(rules.some((rule) => !rule.enabled && rule.locked)).toBe(true);
    expect(uiAuditDirectResponse("refresh_license")).toMatchObject({ valid: true, active_service_features: ["fleet"] });
    expect(uiAuditDirectResponse("fleet_status")).toMatchObject({ connected: false, retrying: false });
    expect(uiAuditDirectResponse("app_check_for_updates_doh")).toMatchObject({ available: false, current_version: "3.5.0" });
    expect(uiAuditDirectResponse("fetch_pro_manifest")).toMatchObject({ version: "3.5.0" });
  });

  test("populates a remote endpoint for list, edit, and connect audits", () => {
    const settings = createUiAuditSettings();

    expect(settings.app.rdpNodes).toEqual([
      {
        id: "audit-rdp",
        label: "Audit Workstation",
        hostname: "192.0.2.25",
        username: "AuditUser",
      },
    ]);
    expect(settings.app.selectedRdpNodeId).toBe("audit-rdp");
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

    const logs = uiAuditDirectResponse("get_log_records") as Array<{ level: string; source: string; occurrences?: number }>;
    expect(logs.filter((record) => record.level === "ERROR" || record.level === "WARN").length).toBeGreaterThan(2);
    expect(new Set(logs.map((record) => record.source)).size).toBeGreaterThan(2);
    expect(logs.some((record) => (record.occurrences ?? 1) > 1)).toBe(true);

    const usbStatus = uiAuditDirectResponse("usb_monitor_status") as { running: boolean; notify: boolean };
    const usbTimelineRows = uiAuditDirectResponse("get_usb_timeline") as { records: Record<string, unknown>; sessions: unknown[] };
    expect(usbStatus).toEqual({ running: true, notify: true });
    expect(Object.keys(usbTimelineRows.records).length).toBeGreaterThan(1);
    expect(usbTimelineRows.sessions.length).toBeGreaterThan(1);
    expect(uiAuditDirectResponse("usb_metering_status")).toBe(true);
    expect(uiAuditDirectResponse("usb_hid_guard_status")).toMatchObject({ running: true, alertCount: 1 });
    expect(uiAuditDirectResponse("get_last_access_tracking_status")).toEqual({
      enabled: false,
      raw_value: 2,
      system_managed: true,
    });

    for (const command of ["argus_dlp_recent", "argus_tamper_recent", "argus_print_usb_recent", "argus_app_usage_recent"]) {
      expect((uiAuditDirectResponse(command) as unknown[]).length).toBeGreaterThan(1);
    }
  });

  test("populates filename, indexed-content, drive, purchase, and drift contracts", () => {
    const settings = createUiAuditSettings();
    const names = uiAuditDirectResponse("search_everything") as { results: Array<{ full_path: string }>; total: number };
    const content = uiAuditDirectResponse("search_content") as Array<{ doc_id: string; snippet: string }>;
    const chunks = uiAuditDirectResponse("content_get_doc") as Array<{ field: string; text: string }>;
    const drives = uiAuditDirectResponse("get_wipe_drive_list") as Array<{ isSystem: boolean; isRemovable: boolean }>;
    const offers = uiAuditDirectResponse("get_purchase_catalog") as Array<{ sku: string; checkoutEligible: boolean }>;

    expect(names.results.length).toBeGreaterThan(1);
    expect(names.total).toBe(names.results.length);
    expect(names.results.every((row) => row.full_path.startsWith("C:\\Audit\\"))).toBe(true);
    expect(content.every((row) => row.doc_id.length > 0 && row.snippet.includes("<mark>"))).toBe(true);
    expect(chunks.some((chunk) => chunk.field === "Body" && chunk.text.length > 40)).toBe(true);
    expect(uiAuditDirectResponse("content_index_status")).toMatchObject({ indexed_docs: 428, pending_docs: 0, is_indexing: false });
    expect(drives.some((drive) => drive.isSystem)).toBe(true);
    expect(drives.some((drive) => drive.isRemovable)).toBe(true);
    expect(offers.some((offer) => offer.sku === "fleet" && offer.checkoutEligible)).toBe(true);
    expect(createUiAuditPendingPurchase({ sku: "fleet", seats: 1 })).toMatchObject({
      purchaseId: "audit-purchase-001",
      sku: "fleet",
      seats: 5,
      amount: 3_000,
      currency: "USD",
    });
    expect(createUiAuditPendingPurchase({ sku: "fleet", seats: 6 })).toMatchObject({ seats: 6, amount: 3_600 });
    expect((uiAuditDirectResponse("get_drift_report") as unknown[]).length).toBeGreaterThan(1);
    expect(settings.app.fileSearch?.roots).toEqual(["C:\\Audit\\Cases", "C:\\Audit\\Evidence"]);
    expect(settings.app.fileSearch?.exclusions).toEqual(["*.tmp", "node_modules"]);
    expect(settings.app.searchHotkey).toBe("Ctrl+Space");
  });
});
