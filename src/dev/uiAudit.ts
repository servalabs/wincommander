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
  features: ["paid", "fleet"],
  base_features: ["paid"],
  active_service_features: ["fleet"],
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

const AUDIT_APP_INVENTORY = structuredClone(DECOY_INVENTORY);
const auditUpdateApp = AUDIT_APP_INVENTORY.manifestApps.find((app) => app.id === "7zip.7zip");
if (auditUpdateApp) {
  auditUpdateApp.installedVersion = "23.01";
  auditUpdateApp.latestVersion = "24.08";
  auditUpdateApp.updateAvailable = true;
}
AUDIT_APP_INVENTORY.pendingUpdates = [{
  id: "7zip.7zip",
  name: "7-Zip",
  installedVersion: "23.01",
  latestVersion: "24.08",
  source: "winget",
  inManifest: true,
}];
AUDIT_APP_INVENTORY.summary.updatesAvailable = 1;

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
  paths: [
    "C:\\Audit\\Recent\\audit-case.pdf.lnk",
    "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\RunMRU",
    "2026-08-01 21:14:52 — audit-tool.exe launched",
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

const AUDIT_FLOW_RULES = [
  {
    id: "audit-usb-flow",
    name: "USB inserted → Notify",
    enabled: true,
    triggers: [{ type: "USBTrigger", mode: "insert" }],
    conditions: [{ type: "TimeCondition", startHour: 8, endHour: 18 }],
    actions: [{ type: "NotifyAction", message: "Audit USB device inserted", severity: "warning" }],
    notes: "Synthetic local rule for exhaustive UI verification.",
    tags: ["audit", "usb"],
    riskLevel: "low",
    schemaVersion: 2,
    source: { kind: "Local" },
    locked: false,
  },
  {
    id: "audit-fleet-ransomware-flow",
    name: "Ransomware signal policy",
    enabled: false,
    triggers: [{ type: "RansomwareMonitorTrigger" }],
    conditions: [],
    actions: [
      { type: "NotifyAction", message: "Ransomware activity detected", severity: "danger" },
      { type: "SignalAction", targetRole: "admins", signalType: "distress" },
    ],
    notes: "Synthetic fleet-locked rule for layout verification.",
    tags: ["audit", "fleet"],
    riskLevel: "high",
    schemaVersion: 2,
    source: { kind: "Fleet", epochVersion: 7 },
    locked: true,
  },
];

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
      rdpNodes: [
        {
          id: "audit-rdp",
          label: "Audit Workstation",
          hostname: "192.0.2.25",
          username: "AuditUser",
        },
      ],
      selectedRdpNodeId: "audit-rdp",
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
      apps: { inventory: AUDIT_APP_INVENTORY },
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
      return {
        installed: true,
        path: "C:\\Audit\\VeraCrypt\\VeraCrypt.exe",
        volumes: [
          { letter: "V:", path: "C:\\Audit\\Vaults\\case-files.hc", type: "Normal" },
          { letter: "W:", path: "C:\\Audit\\Vaults\\plausible-cover.hc", type: "Hidden" },
        ],
      };
    case "Get-SystemEncryptionStatus":
      return { encrypted: true, progress: 100, algorithm: "XTS-AES-256", mode: "TPM + PIN" };
    case "Get-AvailableDriveLetters":
      return { letters: ["X", "Y", "Z"] };
    case "Get-EncryptionPartitions":
      return {
        partitions: [
          { diskNumber: 2, partitionNumber: 1, driveLetter: null, size: "64 GB", sizeBytes: 68719476736, devicePath: "\\\\?\\Volume{audit-encrypted}", busType: "USB", model: "Audit Encrypted SSD" },
          { diskNumber: 3, partitionNumber: 2, driveLetter: "Q", size: "128 GB", sizeBytes: 137438953472, devicePath: "\\\\?\\Volume{audit-mounted}", busType: "NVMe", model: "Audit Evidence Disk" },
        ],
      };
    case "Test-RamDiskInstalled":
      return { installed: true };
    case "Get-RamDiskStatus":
      return {
        installed: true,
        disks: [
          {
            deviceNumber: 1,
            letter: "R:",
            sizeBytes: 2147483648,
            size: "2 GB",
            type: "VM",
            properties: "NTFS · Read/Write",
          },
        ],
      };
    case "Get-SystemRamInfo":
      return {
        totalBytes: 34359738368,
        totalMB: 32768,
        freeBytes: 21474836480,
        freeMB: 20480,
      };
    case "Get-BitLockerVolumes":
      return [
        { mountPoint: "C:", volumeType: "OperatingSystem", volumeStatus: "FullyEncrypted", encryptionMethod: "XtsAes256", protectorTypes: ["Tpm", "RecoveryPassword"], recoveryPasswordPresent: true, backupUsed: true },
        { mountPoint: "D:", volumeType: "Data", volumeStatus: "FullyEncrypted", encryptionMethod: "XtsAes256", protectorTypes: ["Password", "RecoveryPassword"], recoveryPasswordPresent: true, backupUsed: true },
      ];
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
        peers: [
          { ID: "audit-peer", Hostname: "AUDIT-LAPTOP", DNSName: "audit-laptop.audit.mesh", OS: "windows", Online: true, Active: true, Relay: "", LastSeen: "2026-08-01T22:00:00Z", IPs: ["100.64.0.11"], RxBytes: 1048576, TxBytes: 524288, CurAddr: "192.0.2.10:41641", ExitNodeOption: false },
          { ID: "audit-gateway", Hostname: "AUDIT-GATEWAY", DNSName: "audit-gateway.audit.mesh", OS: "linux", Online: false, Active: false, Relay: "sfo", LastSeen: "2026-07-01T12:00:00Z", IPs: ["100.64.0.12"], RxBytes: 7340032, TxBytes: 3145728, CurAddr: "", ExitNodeOption: true },
        ],
      };
    case "Get-InstalledBrowsersJson":
      return { browsers: [{ Name: "Microsoft Edge", Hardened: true }, { Name: "Mozilla Firefox", Hardened: false }] };
    case "Get-AppInventory":
      return AUDIT_APP_INVENTORY;
    case "Test-WingetInstalled":
      return { status: "installed" };
    case "Get-InstalledAppxInventory":
      return {
        apps: [
          { name: "Microsoft.BingNews_8wekyb3d8bbwe", packageFullName: "Microsoft.BingNews_4.55.62231.0_x64__8wekyb3d8bbwe", isProvisioned: true, iconData: null },
          { name: "Microsoft.XboxGamingOverlay_8wekyb3d8bbwe", packageFullName: "Microsoft.XboxGamingOverlay_7.124.5142.0_x64__8wekyb3d8bbwe", isProvisioned: true, iconData: null },
        ],
      };
    case "Test-BcuInstalled":
      return { installed: true, cliPath: "C:\\Audit\\BCU\\bcuninstaller.exe" };
    case "Get-BcuApplicationList":
      return {
        apps: [{
          displayName: "Legacy Audit Toolbar", publisher: "Legacy Audit Vendor", displayVersion: "2.4.1", installDate: "20240115", installLocation: "C:\\Audit\\Programs\\LegacyToolbar", installSource: "C:\\Audit\\Installers", uninstallString: '"C:\\Audit\\Programs\\LegacyToolbar\\uninstall.exe"', quietUninstall: '"C:\\Audit\\Programs\\LegacyToolbar\\uninstall.exe" /S', estimatedSizeKB: 28672, isProtected: false, isSystemComponent: false, isOrphaned: false, isUpdate: false, isValid: true, isRegistered: true, isWebBrowser: false, canQuietUninstall: true, uninstallerKind: "Nsis", is64Bit: "X64", registryKeyName: "LegacyAuditToolbar", registryPath: "HKLM\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\LegacyAuditToolbar", aboutUrl: "https://example.invalid/audit-toolbar", comment: "Synthetic removable desktop program.", displayIcon: "C:\\Audit\\Programs\\LegacyToolbar\\toolbar.exe", iconData: null,
        }],
        totalCount: 1,
        scanTime: "2026-08-01T22:45:00Z",
      };
    case "Test-EdgeInstalled":
    case "Test-OneDriveInstalled":
    case "Get-TeamsStatus":
      return { installed: true };
    case "Get-DebloatWindowsIconData":
      return { icons: {} };
    case "Get-StartupItems":
      return [
        { Name: "Audit Case Indexer", Command: '"C:\\Audit\\Tools\\case-indexer.exe" --background', RamUsageMB: 86, Status: "Running", IsEnabled: true, Source: "Registry", Location: "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run", Recommendation: "Keep", Category: "Productivity", Description: "Indexes synthetic audit case metadata." },
        { Name: "Legacy Audit Helper", Command: '"C:\\Audit\\Legacy\\audit-helper.exe"', RamUsageMB: 0, Status: "Disabled", IsEnabled: false, Source: "Folder", Location: "C:\\Audit\\Startup", Recommendation: "Disable", Category: "Utility", Description: "Representative disabled startup entry." },
      ];
    case "Get-LocalLoginUsers":
      return [
        { name: "AuditAnalyst", fullName: "Audit Analyst", description: "Synthetic standard account", enabled: true, hiddenFromLogin: false, builtIn: false, currentUser: true, sid: "S-1-5-21-1000-1000-1000-1001" },
        { name: "AuditService", fullName: "Audit Service", description: "Synthetic hidden service account", enabled: false, hiddenFromLogin: true, builtIn: false, currentUser: false, sid: "S-1-5-21-1000-1000-1000-1002" },
      ];
    case "Get-AllScheduledTasks":
      return [
        { Name: "Audit Case Sync", Path: "\\WinCommander Audit\\", State: "Ready", Description: "Synchronizes synthetic audit case metadata.", Author: "ServaLabs Audit", IsMicrosoft: false, LastRunTime: "2026-08-01T22:30:00Z", NextRunTime: "2026-08-02T22:30:00Z", LastResult: 0 },
        { Name: "Audit Disabled Task", Path: "\\WinCommander Audit\\", State: "Disabled", Description: "Representative disabled scheduled task.", Author: "ServaLabs Audit", IsMicrosoft: false, LastRunTime: null, NextRunTime: null, LastResult: null },
      ];
    case "Get-AllServices":
      return [
        { Name: "WCAuditIndexer", DisplayName: "WinCommander Audit Indexer", Description: "Synthetic running service for UI verification.", StartMode: "Automatic", State: "Running", Status: "OK", CanPauseAndContinue: false, CanStop: true, Recommended: null },
        { Name: "WCAuditLegacy", DisplayName: "WinCommander Audit Legacy Helper", Description: "Synthetic stopped service with a recommendation.", StartMode: "Manual", State: "Stopped", Status: "OK", CanPauseAndContinue: false, CanStop: false, Recommended: "Disabled" },
      ];
    case "Get-NetworkPorts": {
      const rows = [
        { proto: "TCP", localAddr: "192.0.2.10", localPort: 51820, remoteAddr: "198.51.100.24", remotePort: 443, state: "ESTABLISHED", pid: 4242, processName: "audit-sync.exe", processPath: "C:\\Audit\\Tools\\audit-sync.exe" },
        { proto: "UDP", localAddr: "0.0.0.0", localPort: 5353, remoteAddr: "", remotePort: 0, state: "LISTEN", pid: 4343, processName: "audit-discovery.exe", processPath: "C:\\Audit\\Tools\\audit-discovery.exe" },
        { proto: "TCP", localAddr: "127.0.0.1", localPort: 1420, remoteAddr: "", remotePort: 0, state: "LISTEN", pid: 4444, processName: "audit-ui.exe", processPath: "C:\\Audit\\Tools\\audit-ui.exe" },
      ];
      return { status: "ok", durationMs: 18, truncated: false, totals: { tcp: 2, udp: 1, shown: rows.length }, rows };
    }
    case "Get-PhysicalNetworkAdapters":
      return {
        status: "ok",
        adapters: [
          { id: "{AUDIT-WIFI-0001}", groupId: "PCI\\VEN_AUDIT&DEV_WIFI", name: "Audit Wi-Fi", description: "Synthetic Wi-Fi 6E Adapter", kind: "wifi", status: "Up", linkSpeedMbps: "1200", factoryMac: "02-00-00-00-00-01", currentMac: "02-00-00-00-A1-01", isSpoofed: true },
          { id: "{AUDIT-ETH-0002}", groupId: "PCI\\VEN_AUDIT&DEV_ETH", name: "Audit Ethernet", description: "Synthetic 2.5 GbE Adapter", kind: "ethernet", status: "Disconnected", linkSpeedMbps: null, factoryMac: "02-00-00-00-00-02", currentMac: "02-00-00-00-00-02", isSpoofed: false },
        ],
      };
    case "Get-ProtocolBlocks":
      return { blocks: [{ Name: "WinCommander Audit HTTPS Block", Protocol: "TCP", Port: "443", Direction: "Outbound", Enabled: true }] };
    case "Get-AppBranding":
      return { companyName: "ServaLabs", productName: "WinCommander" };
    case "Get-ActivationStatus":
      return {
        windows: { activated: true, edition: "Windows 11 Pro" },
        office: { installed: false },
      };
    case "Get-WCSystemProbe":
    case "Get-WCMigrationData":
      return {};
    default:
      return command.startsWith("Get-") ? structuredClone(AUDIT_TRACE) : { ok: true };
  }
}

export const UI_AUDIT_ARRAY_COMMANDS = [
  "content_get_doc",
  "drop_standard_decoys",
  "f6_list_removable_volumes",
  "flow_list_rules",
  "get_active_alerts",
  "get_auth_anomaly_recent",
  "get_canary_recent",
  "get_drift_report",
  "get_log_records",
  "get_network_honeypot_ports",
  "get_network_honeypot_recent",
  "get_paste_monitor_recent",
  "get_print_audit_log",
  "get_purchase_catalog",
  "get_ransomware_recent",
  "get_ransomware_watched_dirs",
  "get_recent_downloads",
  "get_recent_screen_capture",
  "get_remote_access_recent",
  "get_remote_access_tools",
  "get_top_processes",
  "get_usb_autosandbox_recent",
  "get_usb_hid_alerts",
  "get_usb_storage_volumes",
  "get_usb_transfer_stats",
  "get_wifi_guard_known",
  "get_wifi_guard_recent",
  "get_wipe_drive_list",
  "list_backend_commands",
  "list_canaries",
  "list_decoys",
  "search_content",
] as const;

export const UI_AUDIT_POPULATED_ARRAY_COMMANDS = [
  "drop_standard_decoys",
  "f6_list_removable_volumes",
  "get_active_alerts",
  "get_auth_anomaly_recent",
  "get_canary_recent",
  "get_log_records",
  "get_network_honeypot_ports",
  "get_network_honeypot_recent",
  "get_paste_monitor_recent",
  "get_print_audit_log",
  "get_ransomware_recent",
  "get_ransomware_watched_dirs",
  "get_recent_downloads",
  "get_recent_screen_capture",
  "get_remote_access_recent",
  "get_remote_access_tools",
  "get_top_processes",
  "get_usb_autosandbox_recent",
  "get_usb_hid_alerts",
  "get_usb_storage_volumes",
  "get_usb_transfer_stats",
  "get_wifi_guard_known",
  "get_wifi_guard_recent",
  "list_canaries",
  "list_decoys",
] as const satisfies ReadonlyArray<(typeof UI_AUDIT_ARRAY_COMMANDS)[number]>;

export function uiAuditDirectResponse(command: string): unknown {
  if (command.startsWith("plugin:")) return null;
  switch (command) {
    case "startup_pin_is_configured":
      return false;
    case "is_dev_build":
      return true;
    case "get_license_status":
    case "refresh_license":
    case "activate_license":
    case "start_trial":
      return AUDIT_LICENSE;
    case "get_live_metrics":
      return { cpuUsage: 23, cpuTemp: 47, ramUsagePercent: 38, ramUsedGb: 12.2, ramTotalGb: 32, disks: [{ name: "C:", totalGb: 953.9, freeGb: 542.5 }] };
    case "get_public_ip_trace":
      return { ip: "203.0.113.42" };
    case "get_managed_policy":
      return { managed: false, source: "", values: {} };
    case "flow_list_rules":
      return structuredClone(AUDIT_FLOW_RULES);
    case "list_backend_commands":
      return ["Show-Notification", "Refresh-SystemStatus", "Lock-Workstation"];
    case "get_wipe_drive_list":
    case "get_purchase_catalog":
    case "get_drift_report":
    case "search_content":
    case "content_get_doc":
      return [];
    case "vpn_kill_switch_status":
      return { armed: false, fired: false, tunnelState: "up", lastFiredAt: 0 };
    case "fleet_status":
      return { connected: false, deviceId: "", serverUrl: "", lastEnrollAt: null, lastError: null, retrying: false, pendingApproval: false };
    case "app_check_for_updates_doh":
      return { available: false, version: "3.5.0", current_version: "3.5.0" };
    case "fetch_pro_manifest":
      return {
        version: "3.5.0",
        pub_date: "2026-08-01T20:00:00Z",
        notes: "Synthetic UI audit manifest",
        url: "https://updates.example.invalid/wincommander-pro-3.5.0.exe",
        sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        size: 73400320,
      };
    case "runtime_visibility_state":
      return { state: { version: 1, entries: [{ key: "audit-tray.exe", hiddenAtUnixMs: Date.parse("2026-08-01T21:30:00Z"), applied: true, runValueRenames: [{ subkey: "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run", originalName: "AuditTray", renamedTo: "WCAuditTray" }], uninstallHides: [] }] }, statePath: "C:\\Audit\\runtime-visibility.json" };
    case "scan_runtimes":
      return {
        runtimes: [
          { pid: 4242, parentPid: 1000, name: "audit-tray.exe", exePath: "C:\\Audit\\Tools\\audit-tray.exe", hasVisibleWindow: false, startsAtLogon: true, kind: "TypeB", hideable: true, tags: ["tray", "autostart", "synthetic"] },
          { pid: 4243, parentPid: 4242, name: "audit-worker.exe", exePath: "C:\\Audit\\Tools\\audit-worker.exe", hasVisibleWindow: false, startsAtLogon: false, kind: "TypeC", hideable: true, tags: ["worker", "synthetic"] },
          { pid: 4244, parentPid: 1000, name: "audit-viewer.exe", exePath: "C:\\Audit\\Tools\\audit-viewer.exe", hasVisibleWindow: true, startsAtLogon: false, kind: "TypeE", hideable: false, tags: ["visible", "synthetic"] },
        ],
        scannedAtUnixMs: Date.parse("2026-08-01T22:45:00Z"),
        totalProcesses: 3,
      };
    case "startup_impact_scan":
      return {
        entries: [
          { id: "startup-audit-indexer", name: "Audit Case Indexer", source: "Registry", location: "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run", command: '"C:\\Audit\\Tools\\case-indexer.exe" --background', executablePath: "C:\\Audit\\Tools\\case-indexer.exe", pathExists: true, signatureStatus: "Valid", signer: "ServaLabs Audit", impact: "Medium", recommendation: "Keep" },
          { id: "startup-audit-legacy", name: "Legacy Audit Helper", source: "Startup Folder", location: "C:\\Audit\\Startup", command: '"C:\\Audit\\Legacy\\audit-helper.exe"', executablePath: "C:\\Audit\\Legacy\\audit-helper.exe", pathExists: false, signatureStatus: "NotSigned", signer: null, impact: "High", recommendation: "Disable" },
        ],
        truncated: false,
      };
    case "driver_maintenance_inventory":
      return {
        drivers: [
          { deviceName: "Audit Wi-Fi Adapter", deviceClass: "Net", deviceId: "PCI\\VEN_AUDIT&DEV_WIFI", infName: "oem42.inf", manufacturer: "Audit Hardware Lab", driverVersion: "12.4.2.6", driverDate: "2026-07-14", isSigned: true, signer: "Audit Hardware Lab" },
          { deviceName: "Legacy Audit Filter", deviceClass: "System", deviceId: "ROOT\\AUDITFILTER", infName: "oem99.inf", manufacturer: "Legacy Audit Vendor", driverVersion: "1.0.0.3", driverDate: "2021-04-02", isSigned: false, signer: null },
        ],
        truncated: false,
        cleanupAvailable: false,
        cleanupLimitation: "Driver removal is intentionally unavailable in the UI audit.",
      };
    case "malware_quarantine_list":
      return { entries: [{ quarantineId: "audit-quarantine-001", sha256: "ad8c2d27bc65d97ab1fbc1e1708b31606a39b8c623d6ff52b57ab9c691d17e38", threatLabel: "Audit.Test.Sample", state: "quarantined" }] };
    case "security_threat_snapshot":
      return { source: "local", capturedAt: "2026-08-01T22:45:00Z", defender: { status: "available", realTimeEnabled: true, antivirusEnabled: true, recentThreatCount: 0, severityCounts: {} }, network: { interfaceCount: 2, activeInterfaceCount: 1 } };
    case "security_cve_snapshot":
      return { source: "osv", sourceTimestamp: "2026-08-01T22:45:00Z", queriedVersion: "24H2", status: "requires_approved_windows_provider", results: [], lookupPerformed: false };
    case "metric_alerts_get_config":
      return AUDIT_METRIC_ALERTS;
    case "network_honeypot_status":
      return { running: false, armedPorts: [], conflictingPorts: [], bindAllInterfaces: false };
    case "get_network_honeypot_bind_all_interfaces":
      return false;
    case "get_ping_block_status":
      return { blocked: false };
    case "wifi_guard_status":
      return { running: false, learning: false, knownSsidCount: 1, currentSsid: "Audit Wi-Fi", currentBssid: "00:11:22:33:44:55" };
    case "routine_cleaner_scan":
      return {
        items: [{ id: "audit-cache", category: "applications", label: "Audit application cache", path: "C:\\Audit\\Cache", bytes: 2621440, fileCount: 18, recommended: true, operation: "delete", truncated: false }],
        totalBytes: 2621440,
        totalFiles: 18,
        skippedTargets: 0,
        cancelled: false,
      };
    case "registry_cleaner_scan":
      return { entries: [{ id: "registry-audit-clsid", classId: "{00000000-0000-0000-0000-AUDIT000001}", serverKind: "InprocServer32", missingServer: "C:\\Audit\\Missing\\audit-shell.dll", hive: "HKCU" }], skippedEntries: 1 };
    case "explorer_context_menu_scan":
      return { entries: [{ id: "context-audit-open", label: "Open with Audit Viewer", location: "HKCU\\Software\\Classes\\*\\shell\\AuditViewer", command: '"C:\\Audit\\Missing\\audit-viewer.exe" "%1"', enabled: true }], skippedEntries: 0 };
    case "shortcut_cleaner_scan":
      return { shortcuts: [{ id: "shortcut-audit-legacy", name: "Legacy Audit Helper", path: "C:\\Audit\\Desktop\\Legacy Audit Helper.lnk", target: "C:\\Audit\\Missing\\audit-helper.exe" }], scannedShortcuts: 24, cancelled: false, truncated: false };
    case "environment_cleaner_scan":
      return { entries: [{ id: "environment-audit-path", scope: "User", variable: "PATH", value: "C:\\Audit\\Missing\\bin", kind: "missing-path-entry" }], skippedEntries: 0 };
    case "uninstall_leftovers_scan":
      return { entries: [{ id: "leftover-audit-legacy", name: "Legacy Audit Suite", path: "C:\\Audit\\AppData\\LegacyAudit", bytes: 15728640, scope: "User" }], scannedFolders: 12, skippedFolders: 0, cancelled: false, truncated: false };
    case "get_driver_health":
      return { devices: [{ name: "Legacy Audit Filter", class: "System", status: "Error", problemCode: 28, problemText: "Representative missing-driver warning", severity: "warning", instanceId: "ROOT\\AUDITFILTER\\0000", manufacturer: "Legacy Audit Vendor" }], summary: { total: 1, critical: 0, warning: 1, info: 0, ok: false } };
    case "get_vulnerable_drivers":
      return { vulnerable: [{ filename: "audit-legacy.sys", path: "C:\\Audit\\Drivers\\audit-legacy.sys", state: "present", reason: "Synthetic vulnerable-driver match", matchedBy: "audit-fixture" }], scanned: 2, ok: false };
    case "get_pro_install_status":
      return {
        installed: true,
        install_path: "C:\\Audit\\WinCommander Pro\\wincommander-pro.exe",
        resolved_path: "C:\\Audit\\WinCommander Pro\\wincommander-pro.exe",
        local_version: "3.5.0",
        local_sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      };
    case "get_log_records":
      return [
        { date: "2026-08-01", timestamp: "22:41:09", level: "INFO", source: "core", os: "Windows", message: "UI audit fixture loaded" },
        { date: "2026-08-01", timestamp: "22:42:17", level: "WARN", source: "pro", os: "Windows", message: "Example structured warning for layout verification" },
      ];
    case "get_recent_downloads":
      return [
        { name: "audit-report.pdf", path: "C:\\Audit\\Downloads\\audit-report.pdf", sizeBytes: 3145728, modifiedAt: 1785624000 },
        { name: "evidence-bundle.zip", path: "C:\\Audit\\Downloads\\evidence-bundle.zip", sizeBytes: 9437184, modifiedAt: 1785623100 },
      ];
    case "get_top_processes":
      return [
        { name: "audit-tool.exe", cpuUsage: 12.4, ramMb: 286 },
        { name: "case-indexer.exe", cpuUsage: 7.1, ramMb: 412 },
      ];
    case "get_auth_anomaly_recent":
      return [{ kind: "failedLogonBurst", severity: "high", summary: "8 failed sign-ins in 90 seconds", user: "AuditUser", ip: "192.0.2.44", count: 8, detectedAt: "2026-08-01T22:40:00Z" }];
    case "list_canaries":
      return [{ id: "audit-canary", label: "Quarterly report", tokenType: "docx", outputPath: "C:\\Audit\\Canaries\\Quarterly Report.docx", beaconUrl: "http://127.0.0.1:8765/beacon/audit-canary", createdAt: "2026-08-01T20:00:00Z" }];
    case "get_canary_recent":
      return [{ tokenId: "audit-canary", label: "Quarterly report", remoteAddr: "127.0.0.1", userAgent: "UI audit agent", firedAt: "2026-08-01T22:30:00Z" }];
    case "get_network_honeypot_ports":
      return [{ port: 445, label: "SMB", enabled: true, custom: false }, { port: 3389, label: "RDP", enabled: false, custom: false }];
    case "get_network_honeypot_recent":
      return [{ port: 445, service: "SMB", peer: "192.0.2.55", peekHex: "ff534d42", detectedAt: "2026-08-01T22:35:00Z" }];
    case "get_wifi_guard_known":
      return [["Audit Wi-Fi", ["00:11:22:33:44:55", "00:11:22:33:44:66"]]];
    case "get_wifi_guard_recent":
      return [{ ssid: "Audit Wi-Fi", bssid: "00:11:22:33:44:99", auth: "WPA2", signal: "82%", reason: "newBssid", detectedAt: "2026-08-01T22:37:00Z" }];
    case "f6_list_removable_volumes":
      return [{ driveLetter: "E:", label: "AUDIT USB" }];
    case "list_decoys":
      return [{ path: "C:\\Audit\\Decoys\\Passwords.xlsx", exists: true }];
    case "get_decoy_recent":
      return [{ path: "C:\\Audit\\Decoys\\Passwords.xlsx", kind: "read", detected_at: "2026-08-01T22:32:00Z" }];
    case "drop_standard_decoys":
      return ["C:\\Audit\\Decoys\\Passwords.xlsx", "C:\\Audit\\Decoys\\Payroll.docx"];
    case "get_print_audit_log":
      return [{ timeCreated: "2026-08-01T22:20:00Z", document: "C:\\Audit\\Cases\\report.docx", pages: 12, printer: "Audit Printer", user: "AuditUser" }];
    case "get_paste_monitor_recent":
      return [{ pattern: "Suspicious encoded command", severity: "danger", detected_at: "2026-08-01T22:29:00Z" }];
    case "get_ransomware_recent":
      return [{ count: 48, window_seconds: 10, sample_paths: ["C:\\Audit\\Cases\\sample-a.docx", "C:\\Audit\\Cases\\sample-b.pdf"], detected_at: "2026-08-01T22:25:00Z", pid: 4242, image_name: "audit-tool.exe", image_path: "C:\\Audit\\audit-tool.exe", action_taken: "monitor" }];
    case "get_ransomware_watched_dirs":
      return ["C:\\Audit\\Cases", "C:\\Audit\\Evidence"];
    case "get_remote_access_tools":
      return [{ id: "audit-rdp", label: "Remote Desktop", processNames: ["mstsc.exe"], ports: [3389], enabled: true }];
    case "get_remote_access_recent":
      return [{ tool: "Remote Desktop", confidence: "high", reason: "Incoming RDP session", port: 3389, peer: "192.0.2.64", logHint: "Security 4624", detectedAt: "2026-08-01T22:28:00Z" }];
    case "get_active_alerts":
      return [{ alertId: "audit-alert", kind: "attention", severity: "warn", summary: "Secondary device entered frame", firedAt: "2026-08-01T22:26:00Z" }];
    case "get_usb_storage_volumes":
      return [{ driveLetter: "E:", label: "AUDIT USB", model: "Audit Flash Drive", serial: "AUDIT123" }];
    case "get_usb_transfer_stats":
      return [{ deviceKey: "0781:5581:storage:AUDIT123", friendlyName: "AUDIT USB", readBytes: 7340032, writeBytes: 2097152, lastSampleEpoch: 1785624000 }];
    case "get_usb_hid_alerts":
      return [{ deviceKey: "046d:c31c:hid:AUDITKBD", friendlyName: "Audit Keyboard", detectedAt: "2026-08-01T22:24:00Z", gapsSampled: 24, medianGapMs: 7.2, recentHidDevice: "Audit Keyboard", redFlag: "hidOnly", severity: "warning" }];
    case "get_usb_autosandbox_recent":
      return [{ time: "2026-08-01T22:23:00Z", deviceKey: "0781:5581:storage:AUDIT123", friendlyName: "AUDIT USB", action: "alert", enforced: false, detail: "New removable storage observed" }];
    case "get_recent_screen_capture":
      return [{ tool: "OBS Studio", processName: "obs64.exe", confidence: "high", detectedAt: "2026-08-01T22:22:00Z" }];
    case "get_usb_timeline":
      return {
        records: {
          auditUsb: {
            identity: { key: "0781:5581:storage:AUDIT123", vid: "0781", pid: "5581", friendlyName: "Audit Flash Drive", isHid: false, isMassStorage: true, instanceId: "USBSTOR\\AUDIT123" },
            lastSeen: 1785624000,
            totalPluggedSecs: 1800,
            sessionCount: 3,
          },
        },
        sessions: [{ deviceKey: "0781:5581:storage:AUDIT123", attachedAt: 1785623100, detachedAt: null, volumeLetter: "E:" }],
      };
    default:
      if (command.endsWith("_list") || command.startsWith("list_") || command.includes("_recent")) return [];
      return structuredClone(AUDIT_TRACE);
  }
}

export function installUiAuditMocks(): void {
  mockWindows("main");
  let settings = createUiAuditSettings();
  let flowRules = structuredClone(AUDIT_FLOW_RULES);
  let fleetStatus = {
    connected: false,
    deviceId: "",
    serverUrl: "",
    lastEnrollAt: null as string | null,
    lastError: null as string | null,
    retrying: false,
    pendingApproval: false,
  };
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
    if (command === "flow_list_rules") return structuredClone(flowRules);
    if (command === "flow_save_rule" && args.rule && typeof args.rule === "object") {
      const next = structuredClone(args.rule) as (typeof flowRules)[number];
      const existing = flowRules.findIndex((rule) => rule.id === next.id);
      if (existing >= 0) flowRules[existing] = next;
      else flowRules.push(next);
      return null;
    }
    if (command === "flow_delete_rule") {
      flowRules = flowRules.filter((rule) => rule.id !== args.ruleId);
      return null;
    }
    if (command === "flow_set_enabled") {
      flowRules = flowRules.map((rule) => rule.id === args.ruleId ? { ...rule, enabled: Boolean(args.enabled) } : rule);
      return null;
    }
    if (command === "flow_fire_now") return null;
    if (command === "fleet_status") return structuredClone(fleetStatus);
    if (command === "fleet_connect") {
      fleetStatus = {
        connected: true,
        deviceId: "audit-device-01",
        serverUrl: String(args.serverUrl ?? "https://fleet.audit.invalid"),
        lastEnrollAt: "2026-08-01T22:45:00Z",
        lastError: null,
        retrying: false,
        pendingApproval: true,
      };
      return null;
    }
    if (command === "fleet_disconnect") {
      fleetStatus = { ...fleetStatus, connected: false, retrying: false, pendingApproval: false };
      return null;
    }
    return uiAuditDirectResponse(command);
  }, { shouldMockEvents: true });
}
