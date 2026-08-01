import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { DECOY_APP_SETTINGS, DECOY_INVENTORY } from "../lib/decoyFakeData";
import { modulesForPersona } from "../types/modules";
import type { AppSettings } from "../types/settings";

export const UI_AUDIT_QUERY_KEY = "wc-ui-audit";
export const UI_AUDIT_PANEL_QUERY_KEY = "panel";

const AUDIT_LICENSE = {
  configured: true,
  licensed: true,
  valid: true,
  plan: "Pro",
  features: ["paid"],
  base_features: ["paid"],
  active_service_features: [],
  device_hash: "ui-audit-device",
  seats_used: 1,
  seat_limit: 5,
  trial_active: false,
  trial_available: false,
};

const AUDIT_SYSTEM_INFO = {
  osName: "Windows 11 Pro",
  osVersion: "24H2",
  buildNumber: "26100",
  hostname: "WC-AUDIT-PC",
  deviceType: "Desktop",
  isAdmin: true,
  cpu: "Audit CPU",
  cpuUsage: 23,
  cpuTemp: 47,
  ram: "32 GB",
  ramUsage: 38,
  ramUsedGb: 12.2,
  ramTotalGb: 32,
  gpu: "Audit GPU",
  disks: [
    { id: "C:", label: "C:", totalGb: 953.9, usedGb: 411.4, freeGb: 542.5, percent: 43, healthPercent: 96 },
  ],
  uptime: { days: 2, hours: 7, minutes: 18 },
};

const AUDIT_METRIC_ALERTS = {
  cpu: { enabled: false, threshold: 50, hysteresisEnabled: true, hysteresisPct: 20, sustainedEnabled: true, sustainedSecs: 30 },
  upload: { enabled: false, threshold: 10, hysteresisEnabled: true, hysteresisPct: 20, sustainedEnabled: true, sustainedSecs: 30 },
  download: { enabled: false, threshold: 10, hysteresisEnabled: true, hysteresisPct: 20, sustainedEnabled: true, sustainedSecs: 30 },
};

const AUDIT_TRACE = {
  entries: [
    { name: "audit-tool.exe", path: "C:\\Audit\\audit-tool.exe", source: "ShimCache", lastRun: "2026-08-01 22:31:08", count: 4 },
    { name: "report.docx", path: "C:\\Audit\\Cases\\report.docx", source: "RecentDocs", lastRun: "2026-08-01 21:14:52", count: 1 },
  ],
  devices: [
    { friendlyName: "Audit USB Drive", description: "USB Mass Storage Device", deviceId: "USBSTOR\\AUDIT", firstSeen: "2026-07-28", lastSeen: "2026-08-01" },
  ],
  items: [
    { originalPath: "C:\\Audit\\deleted-note.txt", deletedTime: "2026-08-01 20:04:11", sizeKB: 42, drive: "C:" },
  ],
  files: [
    { name: "audit.wal", path: "C:\\Audit\\audit.db-wal", sizeKB: 128, modified: "2026-08-01 19:48:03", source: "SQLite WAL" },
  ],
  databases: [
    { name: "ukg.db", path: "C:\\Audit\\Recall\\ukg.db", sizeKB: 4096, modified: "2026-08-01 18:23:10", source: "Recall" },
  ],
  records: [
    { category: "Application", id: "AUDIT-001", name: "audit-tool.exe", path: "C:\\Audit\\audit-tool.exe", publisher: "Audit Publisher" },
  ],
  sections: [
    { name: "RunMRU", count: 2, sample: [{ valueName: "a", value: "audit-tool.exe", user: "AuditUser" }] },
  ],
  browsers: [
    { browser: "Edge", profile: "Default", artifact: "History", path: "C:\\Audit\\Edge\\History", count: 12 },
  ],
  total: 2,
  totalSizeKB: 4266,
  hveFileExists: true,
  hveFileSizeMb: 8.4,
  clipboardHistoryDisabled: false,
  cloudClipboardDisabled: true,
  historyItems: [
    { type: "text", preview: "Audit clipboard record", charCount: 22 },
  ],
};

function mergeSettings<T>(current: T, patch: unknown): T {
  if (!patch || typeof patch !== "object" || Array.isArray(patch)) return current;
  const output = { ...(current as Record<string, unknown>) };
  for (const [key, value] of Object.entries(patch as Record<string, unknown>)) {
    const previous = output[key];
    output[key] = value && typeof value === "object" && !Array.isArray(value)
      ? mergeSettings(previous && typeof previous === "object" ? previous : {}, value)
      : value;
  }
  return output as T;
}

export function createUiAuditSettings(): AppSettings {
  return mergeSettings(structuredClone(DECOY_APP_SETTINGS), {
    app: {
      experienceLevel: "advanced",
      threatPersona: "secure",
      modules: modulesForPersona("secure"),
      metricAlerts: AUDIT_METRIC_ALERTS,
      meshHowItWorksSeen: true,
      permanentlyHiddenPanels: [],
      lockedPanelIds: [],
      hiddenSidebarActions: [],
      borrowedHidden: [],
      hideAdvisor: false,
      hideEnginesSection: false,
      hideLicensePanel: false,
      sidebarCollapsed: false,
      lastPanel: "dashboard",
    },
    ideal: {
      identity: {
        branding: { companyName: "ServaLabs", productName: "WinCommander" },
        advancedToolsEnabled: false,
      },
    },
    current: {
      device: {
        hostname: AUDIT_SYSTEM_INFO.hostname,
        osName: AUDIT_SYSTEM_INFO.osName,
        osVersion: AUDIT_SYSTEM_INFO.osVersion,
        buildNumber: AUDIT_SYSTEM_INFO.buildNumber,
        deviceType: AUDIT_SYSTEM_INFO.deviceType,
        isAdmin: true,
        cpu: AUDIT_SYSTEM_INFO.cpu,
        gpu: AUDIT_SYSTEM_INFO.gpu,
        ram: AUDIT_SYSTEM_INFO.ram,
      },
      apps: { inventory: DECOY_INVENTORY },
    },
  });
}

export function uiAuditBackendResponse(command: string): unknown {
  switch (command) {
    case "Get-StartupStatus":
      return { systemInfo: AUDIT_SYSTEM_INFO, productivity: { installed: true, running: true, details: { server: true, input: true, active: true } } };
    case "Get-SystemInfo":
      return AUDIT_SYSTEM_INFO;
    case "Get-DependencyStatus":
      return {
        cacheAgeSecs: 0,
        dependencies: [
          { id: "winget", name: "WinGet", installed: true, running: true, version: "1.10", canStart: false },
          { id: "veracrypt", name: "VeraCrypt", installed: true, running: null, version: "1.26", canStart: false },
          { id: "meshVpn", panelId: "private-mesh", name: "Private Mesh", installed: true, running: true, version: "1.84", canStart: true },
          { id: "activitywatch", name: "ActivityWatch", installed: true, running: true, version: "0.13", canStart: true },
        ],
      };
    case "Get-DefenderStatus":
      return { enabled: true, realtimeEnabled: true, signaturesUpToDate: true, lastQuickScan: "2026-08-01" };
    case "Get-UpdateStatus":
      return { paused: false, pendingCount: 0, rebootRequired: false };
    case "Get-BlocklistStatus":
      return { applied: ["malware", "tracking"], count: 32184 };
    case "Get-DNSStatus":
      return { provider: "Cloudflare", dohId: "cloudflare", servers: ["1.1.1.1", "1.0.0.1"] };
    case "Get-EncryptionStatus":
      return { installed: true, volumes: [], systemDrive: { encrypted: true, status: "Protected" } };
    case "Get-ProductivityStatus":
      return { installed: true, running: true, details: { server: true, input: true, active: true } };
    case "Get-MeshVPNStatus":
      return {
        installed: true,
        running: true,
        backendState: "Running",
        loggedOut: false,
        health: [],
        self: { Hostname: "WC-AUDIT-PC", Online: true, Active: true, IPs: ["100.64.0.10"], ExitNode: false, ExitNodeOption: true },
        prefs: { AdvertiseExitNode: false, ExitNodeIP: "", ExitNodeAllowLANAccess: false, ShieldsUp: false, Unattended: true, AcceptRoutes: true, AcceptDNS: true },
        MagicDNSSuffix: "audit.mesh",
        peers: [{ ID: "audit-peer", HostName: "AUDIT-LAPTOP", DNSName: "audit-laptop.audit.mesh", OS: "windows", Online: true, Active: true, Relay: "", LastSeen: "2026-08-01T22:00:00Z", IPs: ["100.64.0.11"], RxBytes: 1048576, TxBytes: 524288, CurAddr: "192.0.2.10:41641", ExitNodeOption: false }],
      };
    case "Get-InstalledBrowsersJson":
      return { browsers: [{ Name: "Microsoft Edge", Hardened: true }, { Name: "Mozilla Firefox", Hardened: false }] };
    case "Get-AppInventory":
      return DECOY_INVENTORY;
    case "Get-StartupItems":
    case "Get-LocalLoginUsers":
    case "Get-AllScheduledTasks":
    case "Get-AllServices":
      return [];
    case "Get-AppBranding":
      return { companyName: "ServaLabs", productName: "WinCommander" };
    case "Get-WCSystemProbe":
    case "Get-WCMigrationData":
      return {};
    default:
      return command.startsWith("Get-") ? structuredClone(AUDIT_TRACE) : { ok: true };
  }
}

export function uiAuditDirectResponse(command: string): unknown {
  if (command.startsWith("plugin:")) return null;
  switch (command) {
    case "startup_pin_is_configured":
      return false;
    case "is_dev_build":
      return true;
    case "get_license_status":
      return AUDIT_LICENSE;
    case "get_live_metrics":
      return { cpuUsage: 23, cpuTemp: 47, ramUsagePercent: 38, ramUsedGb: 12.2, ramTotalGb: 32, disks: [{ name: "C:", totalGb: 953.9, freeGb: 542.5 }] };
    case "get_managed_policy":
      return { managed: false, source: "", values: {} };
    case "flow_list_rules":
      return [];
    case "runtime_visibility_state":
      return { state: { entries: [] }, statePath: "C:\\Audit\\runtime-visibility.json" };
    case "scan_runtimes":
      return { runtimes: [], scannedAtUnixMs: Date.parse("2026-08-01T22:45:00Z"), totalProcesses: 0 };
    case "startup_impact_scan":
      return { entries: [], truncated: false };
    case "driver_maintenance_inventory":
      return { drivers: [], truncated: false, cleanupAvailable: false, cleanupLimitation: "Driver removal is intentionally unavailable in the UI audit." };
    case "malware_quarantine_list":
      return { entries: [] };
    case "security_threat_snapshot":
      return { source: "local", capturedAt: "2026-08-01T22:45:00Z", defender: { status: "available", realTimeEnabled: true, antivirusEnabled: true, recentThreatCount: 0, severityCounts: {} }, network: { interfaceCount: 2, activeInterfaceCount: 1 } };
    case "security_cve_snapshot":
      return { source: "osv", sourceTimestamp: "2026-08-01T22:45:00Z", queriedVersion: "24H2", status: "requires_approved_windows_provider", results: [], lookupPerformed: false };
    case "metric_alerts_get_config":
      return AUDIT_METRIC_ALERTS;
    case "routine_cleaner_scan":
      return {
        items: [{ id: "audit-cache", category: "applications", label: "Audit application cache", path: "C:\\Audit\\Cache", bytes: 2621440, fileCount: 18, recommended: true, operation: "delete", truncated: false }],
        totalBytes: 2621440,
        totalFiles: 18,
        skippedTargets: 0,
        cancelled: false,
      };
    case "registry_cleaner_scan":
      return { entries: [], skippedEntries: 0 };
    case "explorer_context_menu_scan":
      return { entries: [], skippedEntries: 0 };
    case "shortcut_cleaner_scan":
      return { shortcuts: [], scannedShortcuts: 24, cancelled: false, truncated: false };
    case "environment_cleaner_scan":
      return { entries: [], skippedEntries: 0 };
    case "uninstall_leftovers_scan":
      return { entries: [], scannedFolders: 12, skippedFolders: 0, cancelled: false, truncated: false };
    case "get_driver_health":
      return { devices: [], summary: { total: 0, critical: 0, warning: 0, info: 0, ok: true } };
    case "get_vulnerable_drivers":
      return { vulnerable: [], scanned: 0, ok: true };
    case "get_pro_install_status":
      return { installed: true, compatible: true, version: "3.2.4" };
    case "get_log_records":
      return [
        { date: "2026-08-01", timestamp: "22:41:09", level: "INFO", source: "core", os: "Windows", message: "UI audit fixture loaded" },
        { date: "2026-08-01", timestamp: "22:42:17", level: "WARN", source: "pro", os: "Windows", message: "Example structured warning for layout verification" },
      ];
    default:
      if (command.endsWith("_list") || command.startsWith("list_") || command.includes("_recent")) return [];
      return structuredClone(AUDIT_TRACE);
  }
}

export function installUiAuditMocks(): void {
  mockWindows("main");
  let settings = createUiAuditSettings();
  mockIPC((command, payload) => {
    const args = payload && typeof payload === "object" && !Array.isArray(payload)
      ? payload as Record<string, unknown>
      : {};
    if (command === "get_settings") return structuredClone(settings);
    if (command === "patch_settings_cmd" || command === "set_settings" || command === "update_current_state") {
      const patch = command === "set_settings" ? args.settings : command === "update_current_state" ? { current: args.probe } : args.patch;
      settings = mergeSettings(settings, patch);
      return structuredClone(settings);
    }
    if (command === "run_backend_script") return uiAuditBackendResponse(String(args.command ?? ""));
    return uiAuditDirectResponse(command);
  }, { shouldMockEvents: true });
}
