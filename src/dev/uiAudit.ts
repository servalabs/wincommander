import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { DECOY_APP_SETTINGS, DECOY_INVENTORY } from "../lib/decoyFakeData";
import { modulesForPersona } from "../types/modules";
import type { AppSettings } from "../types/settings";

export const UI_AUDIT_QUERY_KEY = "wc-ui-audit";
export const UI_AUDIT_PANEL_QUERY_KEY = "panel";
export const UI_AUDIT_LICENSE_QUERY_KEY = "license";

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

const AUDIT_UNLICENSED = {
  configured: true,
  licensed: false,
  valid: false,
  reason: "No license is installed on this synthetic audit device.",
  plan: null,
  features: [] as string[],
  base_features: [] as string[],
  active_service_features: [] as string[],
  device_hash: "ui-audit-device",
  seats_used: 0,
  seat_limit: null,
  trial_active: false,
  trial_available: true,
};

export function createUiAuditLicenseStatus(mode?: string | null) {
  return structuredClone(mode === "unlicensed" ? AUDIT_UNLICENSED : AUDIT_LICENSE);
}

export function createUiAuditPendingPurchase(input?: { sku?: string; seats?: number | null }) {
  const requestedSku = input?.sku ?? "pro_lifetime";
  if (requestedSku !== "pro_lifetime" && requestedSku !== "pro_membership") {
    throw new Error("This offer requires a reviewed order and is unavailable in direct checkout.");
  }
  const sku = requestedSku;
  const amount = sku === "pro_membership" ? 3_000 : 33_000;
  return {
    purchaseId: "audit-purchase-001",
    sku,
    seats: null,
    checkoutUrl: "https://checkout.example.invalid/audit-purchase-001",
    amount,
    currency: "USD",
    expiresAt: Date.parse("2026-08-03T00:00:00Z"),
  };
}

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
  ram: { enabled: false, threshold: 80, hysteresisEnabled: true, hysteresisPct: 20, sustainedEnabled: true, sustainedSecs: 30 },
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

const AUDIT_FILE_TRACE = {
  files: [
    { name: "audit-cache.db", path: "C:\\Audit\\Cache\\audit-cache.db", sizeKB: 384, modified: "2026-08-01 19:48:03", source: "Local cache" },
    { name: "audit-cache.db-wal", path: "C:\\Audit\\Cache\\audit-cache.db-wal", sizeKB: 128, modified: "2026-08-01 19:49:11", source: "SQLite WAL" },
  ],
  total: 2,
  totalSizeKB: 512,
  totalSizeMB: 0.5,
};

function cleanupAuditResponse(command: string): unknown | undefined {
  switch (command) {
    case "Get-DnsCacheEntries":
      return {
        entries: [
          { name: "audit.internal", data: "192.0.2.44", recordType: "A", status: "Success", ttl: 245 },
          { name: "casefiles.internal", data: "2001:db8::44", recordType: "AAAA", status: "Success", ttl: 418 },
        ],
        total: 2,
      };
    case "Get-USBDeviceHistory":
      return {
        devices: [
          { deviceId: "USBSTOR\\DISK&VEN_AUDIT&PROD_EVIDENCE_SSD", friendlyName: "Audit Evidence SSD", manufacturer: "ServaLabs Audit", className: "DiskDrive" },
          { deviceId: "USB\\VID_A11D&PID_0002", friendlyName: "Audit Security Key", manufacturer: "ServaLabs Audit", className: "HIDClass" },
        ],
        total: 2,
      };
    case "Get-ExecutionCache":
      return {
        entries: [
          { path: "C:\\Audit\\Tools\\audit-tool.exe", source: "ShimCache" },
          { path: "C:\\Audit\\Tools\\case-indexer.exe", source: "UserAssist" },
        ],
        total: 2,
      };
    case "Get-ProcessIntelligence":
      return {
        entries: [
          { name: "audit-sync.exe", pid: 4242, path: "C:\\Audit\\Tools\\audit-sync.exe", signed: "Valid", signer: "ServaLabs Audit", elevated: "No" },
          { name: "case-indexer.exe", pid: 4343, path: "C:\\Audit\\Tools\\case-indexer.exe", signed: "NotSigned", signer: null, elevated: "Yes" },
        ],
        total: 2,
      };
    case "Get-ShellBags":
      return {
        entries: [
          { path: "C:\\Audit\\Cases", lastModified: "2026-08-01 21:14:52", source: "BagMRU" },
          { path: "D:\\Evidence\\Exports", lastModified: "2026-08-01 20:02:17", source: "Bags" },
        ],
        total: 2,
      };
    case "Get-SRUMData":
      return {
        entries: [
          { name: "audit-sync.exe", pid: 4242, path: "C:\\Audit\\Tools\\audit-sync.exe", owner: "AuditAnalyst", cpuTime: 183.4, memoryKB: 86400, threadCount: 12 },
          { name: "case-indexer.exe", pid: 4343, path: "C:\\Audit\\Tools\\case-indexer.exe", owner: "AuditAnalyst", cpuTime: 92.1, memoryKB: 42112, threadCount: 7 },
        ],
        srumSizeMb: 12.6,
        total: 2,
      };
    case "Get-EventLogSummary":
      return {
        logs: [
          { name: "System", count: 1824, oldest: "2026-07-28 09:12:00", newest: "2026-08-01 22:41:08", sizeMb: 20.1 },
          { name: "Security", count: 3641, oldest: "2026-07-25 18:05:22", newest: "2026-08-01 22:43:51", sizeMb: 64.0 },
        ],
        total: 5465,
      };
    case "Get-PSHistory":
      return {
        entries: [
          { id: 1, command: "Get-FileHash C:\\Audit\\Cases\\report.docx" },
          { id: 2, command: "Get-WinEvent -LogName System -MaxEvents 25" },
        ],
        historyPath: "C:\\Users\\AuditAnalyst\\AppData\\Roaming\\Microsoft\\Windows\\PowerShell\\PSReadLine\\ConsoleHost_history.txt",
        total: 2,
        fileTotal: 1,
      };
    case "Get-RecentFiles":
      return {
        entries: [
          { name: "report.docx", extension: ".docx", target: "C:\\Audit\\Cases\\report.docx", lastModified: "2026-08-01 21:14:52", sizeBytes: 65536 },
          { name: "timeline.csv", extension: ".csv", target: "C:\\Audit\\Exports\\timeline.csv", lastModified: "2026-08-01 20:48:03", sizeBytes: 18432 },
        ],
        total: 2,
        path: "C:\\Users\\AuditAnalyst\\AppData\\Roaming\\Microsoft\\Windows\\Recent",
      };
    case "Get-RDPHistory":
      return {
        entries: [
          { type: "MRU", host: "audit-workstation.internal", username: "AuditAnalyst", key: "MRU0", lastModified: "2026-08-01 18:22:09" },
          { type: "SavedCredential", host: "audit-gateway.internal", username: "AuditReviewer", key: "TERMSRV/audit-gateway.internal", lastModified: "2026-07-31 16:05:44" },
        ],
        total: 2,
      };
    case "Get-ConnectivityHistory":
      return {
        entries: [
          { source: "NetworkList", type: "Profile", name: "Audit Lab", key: "{AUDIT-NETWORK-1}", description: "Private audit network", dnsSuffix: "audit.internal", category: 1 },
          { source: "Signatures", type: "Signature", name: "Audit VPN", key: "{AUDIT-NETWORK-2}", description: "Synthetic VPN signature", dnsSuffix: "vpn.audit.internal", category: 1 },
        ],
        total: 2,
      };
    case "Get-JumpLists":
      return {
        entries: [
          { name: "1b4dd67f29cb1962.automaticDestinations-ms", type: "Automatic", sizeKB: 48, lastModified: "2026-08-01 21:11:09" },
          { name: "5f7b5f1e01b83767.customDestinations-ms", type: "Custom", sizeKB: 12, lastModified: "2026-08-01 20:37:42" },
        ],
        total: 2,
      };
    case "Get-BrowserFootprints":
      return {
        browsers: [
          { browser: "Edge", profilePath: "C:\\Users\\AuditAnalyst\\AppData\\Local\\Microsoft\\Edge\\User Data\\Default", artifacts: [{ name: "History", sizeKB: 2048 }, { name: "Cookies", sizeKB: 512 }], totalSizeKB: 2560 },
          { browser: "Firefox", profilePath: "C:\\Users\\AuditAnalyst\\AppData\\Roaming\\Mozilla\\Firefox\\Profiles\\audit.default", artifacts: [{ name: "places.sqlite", sizeKB: 3072 }], totalSizeKB: 3072 },
        ],
        totalBrowsers: 2,
      };
    case "Get-PrefetchFiles":
      return {
        entries: [
          { name: "AUDIT-TOOL.EXE", fileName: "AUDIT-TOOL.EXE-A11D0001.pf", sizeKB: 42, lastRun: "2026-08-01 22:31:08", created: "2026-07-28 08:12:00" },
          { name: "CASE-INDEXER.EXE", fileName: "CASE-INDEXER.EXE-A11D0002.pf", sizeKB: 36, lastRun: "2026-08-01 22:28:44", created: "2026-07-27 15:41:00" },
        ],
        total: 2,
        path: "C:\\Windows\\Prefetch",
        accessDenied: false,
        enablePrefetcher: 3,
      };
    case "Get-ShadowCopies":
      return {
        copies: [
          { id: "{AUDIT-SHADOW-1}", drive: "C:\\", deviceObject: "\\\\?\\GLOBALROOT\\Device\\HarddiskVolumeShadowCopy42", created: "2026-08-01 03:00:00", originatingMachine: "WC-AUDIT-PC", serviceMachine: "WC-AUDIT-PC", clientAccessible: true, persistent: true, stateStr: "Created" },
        ],
        total: 1,
        vssRunning: true,
      };
    case "Get-NTFSJournals":
      return {
        journals: [
          { drive: "C:", present: true, journalId: "0x01DAA11D", maxSize: "32 MB" },
          { drive: "D:", present: true, journalId: "0x01DAA11E", maxSize: "64 MB" },
        ],
        total: 2,
      };
    case "Get-AmcacheEntries":
      return {
        entries: [
          { category: "Application", count: 2, sample: [{ id: "AUDIT-APP-1", name: "audit-tool.exe", path: "C:\\Audit\\Tools\\audit-tool.exe" }, { id: "AUDIT-APP-2", name: "case-indexer.exe", path: "C:\\Audit\\Tools\\case-indexer.exe" }] },
        ],
        hveFileSizeMb: 8.4,
        hveFileExists: true,
        total: 2,
      };
    case "Get-NTUserTraces":
      return {
        sections: [
          { name: "RunMRU", count: 2, entries: [{ key: "a", value: "audit-tool.exe" }, { key: "b", value: "case-indexer.exe" }] },
          { name: "RecentDocs", count: 1, entries: [{ key: ".docx", value: "report.docx" }] },
        ],
        total: 3,
      };
    case "Get-CrashDumpList":
      return {
        dumps: [
          { source: "LocalDumps", name: "audit-tool.4242.dmp", sizeKB: 2048, modified: "2026-08-01 17:08:22" },
          { source: "WER", name: "case-indexer.4343.dmp", sizeKB: 1536, modified: "2026-07-31 11:42:03" },
        ],
        total: 2,
        totalSizeMB: 3.5,
      };
    case "Get-SQLiteWALList":
      return {
        files: [
          { name: "audit.db-wal", sizeKB: 128, dir: "C:\\Audit\\Cache", modified: "2026-08-01 19:49:11" },
          { name: "timeline.db-shm", sizeKB: 32, dir: "C:\\Audit\\Timeline", modified: "2026-08-01 19:49:08" },
        ],
        total: 2,
        totalSizeMB: 0.16,
      };
    case "Get-RecallDatabaseInfo":
      return {
        databases: [
          { source: "Recall", name: "ukg.db", path: "C:\\Audit\\Recall\\ukg.db", sizeKB: 4096, modified: "2026-08-01 18:23:10" },
          { source: "Timeline", name: "ActivitiesCache.db", path: "C:\\Audit\\Timeline\\ActivitiesCache.db", sizeKB: 1536, modified: "2026-08-01 18:19:44" },
        ],
        total: 2,
        totalSizeMB: 5.5,
      };
    case "Get-RecycleBinInfo":
      return {
        items: [
          { originalPath: "C:\\Audit\\deleted-note.txt", deletedTime: "2026-08-01 20:04:11", sizeBytes: 43008, sizeKB: 42, account: "AuditAnalyst", sid: "S-1-5-21-1000-1000-1000-1001", drive: "C:" },
          { originalPath: "D:\\Evidence\\draft.csv", deletedTime: "2026-08-01 18:52:44", sizeBytes: 18432, sizeKB: 18, account: "AuditReviewer", sid: "S-1-5-21-1000-1000-1000-1002", drive: "D:" },
        ],
        total: 2,
        totalSizeKB: 60,
      };
    case "Get-ClipboardHistoryStatus":
      return {
        clipboardHistoryDisabled: false,
        cloudClipboardDisabled: true,
        historyItems: [
          { type: "text", preview: "Audit clipboard record", charCount: 22 },
          { type: "html", preview: "Case evidence table", charCount: 148 },
        ],
        total: 2,
      };
    case "Get-WlanProfiles":
      return { profiles: [{ name: "Audit Lab", password: "audit-fixture-only" }, { name: "Audit Guest", password: null }], total: 2 };
    case "Get-NetworkDrives":
      return { drives: [{ Name: "Z:", DisplayRoot: "\\\\audit-server\\cases" }, { Name: "Y:", DisplayRoot: "\\\\audit-server\\exports" }], total: 2 };
    case "Get-BluetoothDevices":
      return { devices: [{ id: "BTHENUM\\AUDIT-HEADSET", name: "Audit Headset", lastSeen: "2026-08-01 21:02:18" }], total: 1 };
    case "Get-AutoEraseSchedules":
      return { schedules: [], total: 0 };
    case "Get-AutoEraseSupportedCategories":
      return { categories: [], total: 0 };
    case "Get-PrivacyProtectionStatus":
      return { enabled: false, pagefile: false, prefetch: true, recentFiles: true, jumpLists: true, thumbnailCache: true };
    default:
      break;
  }

  if (/^Get-(?:NotepadStateFiles|PCAInfo|SearchIndexInfo|PrintSpoolerInfo|WebCacheInfo|ThumbnailCacheInfo|NotificationDatabaseInfo|BranchCacheInfo|EventTranscriptInfo|ActivitiesTimelineInfo|RdpBitmapCacheInfo|ServicingLogsInfo|DeviceInstallLogsInfo|UsageTraceLogsInfo|DefenderHistoryInfo|WSLDataInfo|DockerDesktopDataInfo|VirtualMachineArtifactsInfo|DeveloperCachesInfo|CredentialManagerInfo|NetworkWizardHistoryInfo|WERHistoryInfo|InactiveUserProtectionMetadataInfo|StickyNotesInfo|OneDriveMetadataInfo|SpotlightCacheInfo|FontCacheInfo|LegacyIconCacheInfo|GameCapturesInfo|PhotosCacheInfo|XboxCacheInfo|CommunicationCachesInfo|EditorHistoryInfo|GitActivityInfo|SSHStateInfo|RemoteAccessLogsInfo|PasswordManagerCachesInfo|GameLauncherLogsInfo|AdobeRecentInfo|OfficeTempFilesInfo|FirewallLogInfo|NeighborCacheInfo|NetBIOSCacheInfo|GeolocationCacheInfo|VPNPhonebooksInfo|ProxyCacheInfo|CloudPlaceholdersInfo|BITSQueueInfo|CellularHistoryInfo|AppLaunchHistoryInfo|OfficeMruInfo|EmbeddedWebCacheInfo|P2PUpdateCacheInfo|ReliabilityHistoryInfo|ExplorerSearchHistoryInfo|SearchPersonalizationInfo)$/.test(command)) {
    return structuredClone(AUDIT_FILE_TRACE);
  }

  return undefined;
}

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
      hideSidebarPreferences: false,
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
      searchHotkey: "Ctrl+Space",
      fileSearch: {
        roots: ["C:\\Audit\\Cases", "C:\\Audit\\Evidence"],
        exclusions: ["*.tmp", "node_modules"],
      },
    },
    ideal: {
      identity: {
        branding: { companyName: "ServaLabs", productName: "WinCommander" },
        advancedToolsEnabled: false,
      },
      privacy: {
        pasteMonitorCryptoSwapEnabled: true,
        decoyMonitor: {
          enabled: true,
          enrolledPaths: ["C:\\Audit\\Decoys\\Passwords.xlsx"],
        },
        ransomwareMonitor: {
          enabled: true,
          threshold: 50,
          windowSeconds: 30,
          customWatchDirs: ["C:\\Audit\\Cases"],
          action: "monitor",
        },
        remoteAccessMonitor: {
          enabled: true,
          tools: { "audit-rdp": true },
        },
        screenCapture: {
          detectionEnabled: true,
          protectWindow: false,
        },
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
    case "Get-UserProfiles":
      return {
        profiles: [
          {
            name: "AuditAnalyst",
            displayName: "Audit Analyst",
            path: "C:\\Users\\AuditAnalyst",
            sid: "S-1-5-21-1000-1000-1000-1001",
            isCurrent: true,
          },
          {
            name: "AuditReviewer",
            displayName: "Audit Reviewer",
            path: "C:\\Users\\AuditReviewer",
            sid: "S-1-5-21-1000-1000-1000-1002",
            isCurrent: false,
          },
        ],
        total: 2,
        currentUser: "AuditAnalyst",
        currentSid: "S-1-5-21-1000-1000-1000-1001",
        isAdmin: true,
      };
    case "Get-FleetAccessUsers":
      return {
        users: [
          { name: "AuditAnalyst", displayName: "Audit Analyst", sid: "S-1-5-21-1000-1000-1000-1001", isCurrent: true },
          { name: "AuditReviewer", displayName: "Audit Reviewer", sid: "S-1-5-21-1000-1000-1000-1002", isCurrent: false },
        ],
        total: 2,
      };
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
    case "Get-DiskCleanupScan":
      return {
        categories: [
          { Id: "userTemp", Label: "User temporary files", Path: "C:\\Users\\Audit\\AppData\\Local\\Temp", Exists: true, FileCount: 184, SizeBytes: 398_458_880, SizeMb: 380.0 },
          { Id: "windowsTemp", Label: "Windows temporary files", Path: "C:\\Windows\\Temp", Exists: true, FileCount: 71, SizeBytes: 197_656_576, SizeMb: 188.5 },
          { Id: "updateCache", Label: "Windows Update cache", Path: "C:\\Windows\\SoftwareDistribution\\Download", Exists: true, FileCount: 42, SizeBytes: 1_395_864_576, SizeMb: 1_331.2 },
          { Id: "prefetch", Label: "Prefetch cache", Path: "C:\\Windows\\Prefetch", Exists: true, FileCount: 96, SizeBytes: 63_753_216, SizeMb: 60.8 },
          { Id: "windowsOld", Label: "Previous Windows installation", Path: "C:\\Windows.old", Exists: true, FileCount: 2_418, SizeBytes: 13_743_895_347, SizeMb: 13_107.2 },
          { Id: "deliveryOptimization", Label: "Delivery Optimization cache", Path: "C:\\Windows\\SoftwareDistribution\\DeliveryOptimization", Exists: false, FileCount: 0, SizeBytes: 0, SizeMb: 0 },
        ],
      };
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
    default: {
      const cleanupResponse = cleanupAuditResponse(command);
      if (cleanupResponse !== undefined) return cleanupResponse;
      return command.startsWith("Get-") ? structuredClone(AUDIT_TRACE) : { ok: true };
    }
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
  "vault_list_authorized_entries",
] as const;

export const UI_AUDIT_POPULATED_ARRAY_COMMANDS = [
  "content_get_doc",
  "drop_standard_decoys",
  "f6_list_removable_volumes",
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
  "list_canaries",
  "list_decoys",
  "search_content",
  "vault_list_authorized_entries",
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
    case "get_vault_access_capabilities":
      return { can_manage_policy: true };
    case "get_vault_access_policy":
      return {
        schema_version: 1,
        policy_id: "audit-vault-policy",
        version: 4,
        expected_previous_version: 3,
        entries: [{
          id: "audit-team-vault",
          label: "Team documents",
          container_path: "D:\\Vaults\\Team\\team-documents.hc",
          container_kind: "standard",
          owner_account: "AuditAnalyst",
          grants: [
            { principal_name: "AuditAnalyst", access: "write" },
            { principal_name: "AuditReviewer", access: "write" },
          ],
          mount: { presentation: "machine", preferred_letter: "V" },
        }],
      };
    case "get_vault_access_status":
    case "apply_vault_access_policy":
      return {
        policy_id: "audit-vault-policy",
        version: 4,
        validation_state: "current",
        applied_at: Date.parse("2026-08-01T22:40:00Z") / 1000,
        entries: [{ id: "audit-team-vault", result: "applied", mount_state: "unmounted" }],
      };
    case "vault_list_authorized_entries":
      return [{
        entry_id: "audit-team-vault",
        label: "Team documents",
        access: "write",
        presentation: "machine",
        container_kind: "standard",
        mount_state: "unmounted",
        drive_letter: null,
      }];
    case "flow_list_rules":
      return structuredClone(AUDIT_FLOW_RULES);
    case "list_backend_commands":
      return ["Show-Notification", "Refresh-SystemStatus", "Lock-Workstation"];
    case "get_wipe_drive_list":
      return [
        { letter: "C", label: "Windows", freeGB: 542.5, totalGB: 953.9, mediaType: "NVMe", busType: "NVMe", isRemovable: false, isSystem: true },
        { letter: "E", label: "AUDIT USB", freeGB: 42.1, totalGB: 57.7, mediaType: "Unknown", busType: "USB", isRemovable: true, isSystem: false },
      ];
    case "get_purchase_catalog":
      return [
        { sku: "pro_lifetime", name: "Pro Lifetime", priceLabel: "$330 once", detail: "Pro forever with lifetime updates.", deviceRule: "3 transferable active activation/update/service slots; a released PC keeps its last signed normal-Pro fallback", checkoutEligible: true },
        { sku: "pro_membership", name: "Pro Membership", priceLabel: "$30/month", detail: "Pro updates and Netwall included.", deviceRule: "3 transferable active activation/update/service slots; a released PC keeps its last signed normal-Pro fallback", checkoutEligible: true },
        { sku: "investigator", name: "Investigator", priceLabel: "Contact ServaLabs", detail: "Pro, Investigator Mode, and Netwall included.", deviceRule: "3 transferable active activation/update/service slots; a released PC keeps its last signed normal-Pro fallback", checkoutEligible: false, checkoutMessage: "Investigator requires identity, authority, territory, and end-use review before quotation." },
        { sku: "fleet", name: "Fleet", priceLabel: "Contact ServaLabs", detail: "Pro, Fleet management, and Netwall. Investigator is not included.", deviceRule: "one managed Windows endpoint per paid Fleet seat", checkoutEligible: false, checkoutMessage: "Fleet requires an organization, deployment, territory, and end-use review before quotation.", minSeats: 1, maxSeats: 50, seatPricingLabel: "1-15: $25/seat/month. 16-50: $17/additional seat/month. 51+: contact sales." },
      ];
    case "get_drift_report":
      return [
        { path: "ideal.privacy.telemetry", idealValue: false, currentValue: true, command: "Disable-Telemetry" },
        { path: "ideal.network.dnsProvider", idealValue: "cloudflare", currentValue: "automatic", command: "Set-CloudflareDNS" },
      ];
    case "search_content":
      return [
        { doc_id: "1785624000001", path: "C:\\Audit\\Cases\\incident-report.pdf", name: "incident-report.pdf", ext: "pdf", mtime: 1785624000, size: 3145728, score: 0.94, match_kind: "Hybrid", snippet: "The <mark>audit</mark> timeline records the initial alert.", author: "Audit Team", doc_title: "Incident Review", tags: "audit,incident" },
        { doc_id: "1785624000002", path: "C:\\Audit\\Evidence\\collection-notes.docx", name: "collection-notes.docx", ext: "docx", mtime: 1785623100, size: 1048576, score: 0.82, match_kind: "Keyword", snippet: "Validated the <mark>audit</mark> collection against the manifest.", author: "Examiner", doc_title: "Collection Notes", tags: "evidence" },
      ];
    case "content_get_doc":
      return [
        { doc_id: "1785624000001", field: "Title", ordinal: 0, text: "Incident Review" },
        { doc_id: "1785624000001", field: "Body", ordinal: 1, text: "The audit timeline records the initial alert, containment steps, and verification outcome." },
      ];
    case "search_everything":
      return {
        results: [
          { name: "audit-index.csv", directory: "C:\\Audit\\Cases", full_path: "C:\\Audit\\Cases\\audit-index.csv", size: "524288", modified: "2026-08-01 22:40:00", icon_data: null },
          { name: "audit-viewer.exe", directory: "C:\\Audit\\Tools", full_path: "C:\\Audit\\Tools\\audit-viewer.exe", size: "7340032", modified: "2026-08-01 22:25:00", icon_data: null },
        ],
        total: 2,
        query: "audit",
      };
    case "search_everything_count":
      return 2;
    case "content_index_status":
      return { indexed_docs: 428, pending_docs: 0, is_indexing: false, last_error: null, index_size_bytes: 8388608 };
    case "get_pending_purchase":
      return null;
    case "create_purchase":
    case "resume_purchase_checkout":
      return createUiAuditPendingPurchase();
    case "poll_purchase_status":
    case "reconcile_purchase_status":
      return { state: "checkout_pending", providerStatus: "created", amount: 7_900, currency: "USD", activated: false };
    case "cancel_purchase_subscription":
      return Date.parse("2026-09-01T00:00:00Z") / 1000;
    case "vpn_kill_switch_status":
      return { armed: false, fired: false, tunnelState: "up", lastFiredAt: 0 };
    case "usb_monitor_status":
      return { running: true, notify: true };
    case "usb_metering_status":
      return true;
    case "usb_hid_guard_status":
      return { running: true, alertCount: 1 };
    case "usb_autosandbox_status":
      return { running: true, mode: "observe", recentCount: 1 };
    case "usb_device_trust_score":
      return {
        deviceKey: "0781:5581:storage:AUDIT123",
        score: 62,
        signals: { serialStable: true, isHid: false, isMassStorage: true, knownVendor: true, hidAlerts: 0, quarantineActions: 0, transferBytes: 9437184 },
      };
    case "remote_access_monitor_status":
      return { running: true, watchingTools: 1, triggered: true };
    case "get_last_access_tracking_status":
      return { enabled: false, raw_value: 2, system_managed: true };
    case "screen_capture_watch_status":
      return { running: true, lastTick: "2026-08-01T22:45:00Z" };
    case "get_print_audit_status":
      return { channelEnabled: true, channelPresent: true };
    case "canary_listener_status":
      return { running: true, port: 8765 };
    case "argus_app_usage_status":
      return { running: true, startedAt: "2026-08-01T21:00:00Z", intervalMs: 10_000 };
    case "argus_dlp_status":
    case "argus_tamper_status":
    case "argus_print_usb_status":
      return { running: true, startedAt: "2026-08-01T21:00:00Z" };
    case "argus_app_usage_recent":
      return [
        { windowStart: "2026-08-01T22:30:00Z", windowEnd: "2026-08-01T22:45:00Z", activeSeconds: 780, idleSeconds: 120, topCategory: "development", categoryScores: { development: 0.72, communication: 0.18, other: 0.1 } },
        { windowStart: "2026-08-01T22:15:00Z", windowEnd: "2026-08-01T22:30:00Z", activeSeconds: 640, idleSeconds: 260, topCategory: "communication", categoryScores: { communication: 0.61, development: 0.29, other: 0.1 } },
      ];
    case "argus_dlp_recent":
      return [
        { windowStart: "2026-08-01T22:30:00Z", windowEnd: "2026-08-01T22:45:00Z", kind: "usb_transfer", class: "removable_media", magnitude: 9437184, severity: "warning" },
        { windowStart: "2026-08-01T22:15:00Z", windowEnd: "2026-08-01T22:30:00Z", kind: "clipboard_pattern", class: "credential_signal", magnitude: 2, severity: "high" },
      ];
    case "argus_tamper_recent":
      return [
        { windowStart: "2026-08-01T22:30:00Z", windowEnd: "2026-08-01T22:45:00Z", kind: "log_clear_attempt", class: "evidence_integrity", magnitude: 1, severity: "high" },
        { windowStart: "2026-08-01T22:15:00Z", windowEnd: "2026-08-01T22:30:00Z", kind: "sidecar_restart", class: "runtime_integrity", magnitude: 1, severity: "warning" },
      ];
    case "argus_print_usb_recent":
      return [
        { windowStart: "2026-08-01T22:30:00Z", windowEnd: "2026-08-01T22:45:00Z", kind: "print", class: "paper_output", magnitude: 12, severity: "warning" },
        { windowStart: "2026-08-01T22:15:00Z", windowEnd: "2026-08-01T22:30:00Z", kind: "removable_media", class: "device_attach", magnitude: 1, severity: "info" },
      ];
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
        items: [
          { id: "audit-edge-cache", category: "browsers", label: "Microsoft Edge cache", path: "C:\\Audit\\Browser\\Edge\\Cache", bytes: 8_388_608, fileCount: 128, recommended: true, operation: "delete", truncated: false },
          { id: "audit-firefox-thumbnails", category: "browsers", label: "Firefox thumbnail cache", path: "C:\\Audit\\Browser\\Firefox\\thumbnails", bytes: 1_310_720, fileCount: 42, recommended: true, operation: "delete", truncated: false },
          { id: "audit-app-cache", category: "applications", label: "Audit application cache", path: "C:\\Audit\\Cache", bytes: 2_621_440, fileCount: 18, recommended: true, operation: "delete", truncated: false },
          { id: "audit-shader-cache", category: "gaming", label: "DirectX shader cache", path: "C:\\Audit\\Gaming\\D3DCache", bytes: 67_108_864, fileCount: 320, recommended: true, operation: "delete", truncated: false },
          { id: "audit-launcher-logs", category: "gaming", label: "Game launcher logs", path: "C:\\Audit\\Gaming\\Launcher\\Logs", bytes: 786_432, fileCount: 12, recommended: false, operation: "delete", truncated: false },
          { id: "audit-sqlite-journals", category: "databases", label: "Orphaned SQLite journals", path: "C:\\Audit\\Databases", bytes: 3_670_016, fileCount: 7, recommended: false, operation: "delete", truncated: false },
        ],
        totalBytes: 83_886_080,
        totalFiles: 527,
        skippedTargets: 0,
        cancelled: false,
      };
    case "run_disk_scan":
      return {
        scanRoot: "C:\\Audit",
        totalSize: 40_802_189_312,
        freeSpace: 496_068_722_688,
        driveCapacity: 1_099_511_627_776,
        fileCount: 8_421,
        folderCount: 614,
        wiztreeFound: true,
      };
    case "get_disk_children":
      return [
        { name: "Cases", fullPath: "C:\\Audit\\Cases", size: 19_327_352_832, allocated: 19_341_262_848, isDir: true, lastModified: "2026-08-01T22:20:00Z", fileCount: 3_216, folderCount: 188 },
        { name: "Downloads", fullPath: "C:\\Audit\\Downloads", size: 9_663_676_416, allocated: 9_671_524_352, isDir: true, lastModified: "2026-08-01T21:48:00Z", fileCount: 804, folderCount: 37 },
        { name: "AppData", fullPath: "C:\\Audit\\AppData", size: 6_442_450_944, allocated: 6_451_462_144, isDir: true, lastModified: "2026-08-01T22:36:00Z", fileCount: 4_288, folderCount: 389 },
        { name: "evidence-image.iso", fullPath: "C:\\Audit\\evidence-image.iso", size: 5_368_709_120, allocated: 5_368_709_120, isDir: false, lastModified: "2026-07-31T19:04:00Z", fileCount: 1, folderCount: 0 },
      ];
    case "get_large_disk_items":
      return [
        { name: "evidence-image.iso", fullPath: "C:\\Audit\\evidence-image.iso", size: 5_368_709_120, allocated: 5_368_709_120, isDir: false, lastModified: "2026-07-31T19:04:00Z", fileCount: 1, folderCount: 0, itemType: "Archive", cleanupHint: "Review before deleting", risk: "review" },
        { name: "case-archive.zip", fullPath: "C:\\Audit\\Cases\\case-archive.zip", size: 2_576_980_377, allocated: 2_577_006_592, isDir: false, lastModified: "2026-08-01T20:12:00Z", fileCount: 1, folderCount: 0, itemType: "Archive", cleanupHint: "Review before deleting", risk: "review" },
        { name: "capture-session.mkv", fullPath: "C:\\Audit\\Cases\\capture-session.mkv", size: 1_395_864_576, allocated: 1_395_867_648, isDir: false, lastModified: "2026-08-01T18:52:00Z", fileCount: 1, folderCount: 0, itemType: "Video", cleanupHint: "Large media file", risk: "review" },
        { name: "BrowserCache", fullPath: "C:\\Audit\\AppData\\BrowserCache", size: 754_974_720, allocated: 756_023_296, isDir: true, lastModified: "2026-08-01T22:36:00Z", fileCount: 944, folderCount: 48, itemType: "Folder", cleanupHint: "Cache candidate", risk: "temporary" },
      ];
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
        { date: "2026-08-01", timestamp: "22:43:51", level: "ERROR", source: "ui", os: "Windows", message: "Audit panel boundary captured a representative rendering failure with a detailed component stack." },
        { date: "2026-08-01", timestamp: "22:42:17", level: "WARN", source: "pro", os: "Windows", message: "Sidecar connection exceeded the expected response window; retry remains available." },
        { date: "2026-08-01", timestamp: "22:41:42", level: "ERROR", source: "core", os: "Windows", message: "Settings snapshot verification rejected a stale revision and preserved the last known-good copy.", occurrences: 3, firstSeen: "2026-08-01 22:40:58", lastSeen: "2026-08-01 22:41:42" },
        { date: "2026-08-01", timestamp: "22:41:09", level: "INFO", source: "core", os: "Windows", message: "UI audit fixture loaded" },
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
          auditKeyboard: {
            identity: { key: "046d:c31c:hid:AUDITKBD", vid: "046d", pid: "c31c", friendlyName: "Audit Keyboard", isHid: true, isMassStorage: false, instanceId: "HID\\AUDITKBD" },
            lastSeen: 1785623700,
            totalPluggedSecs: 7200,
            sessionCount: 8,
          },
        },
        sessions: [
          { deviceKey: "0781:5581:storage:AUDIT123", attachedAt: 1785623100, detachedAt: null, volumeLetter: "E:" },
          { deviceKey: "046d:c31c:hid:AUDITKBD", attachedAt: 1785616500, detachedAt: null, volumeLetter: null },
        ],
      };
    default:
      if (command.endsWith("_list") || command.startsWith("list_") || command.includes("_recent")) return [];
      return structuredClone(AUDIT_TRACE);
  }
}

export function installUiAuditMocks(): void {
  mockWindows("main");
  let settings = createUiAuditSettings();
  let licenseStatus = createUiAuditLicenseStatus(
    new URLSearchParams(window.location.search).get(UI_AUDIT_LICENSE_QUERY_KEY),
  );
  let pendingPurchase: ReturnType<typeof createUiAuditPendingPurchase> | null = null;
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
    if (command === "get_license_status" || command === "refresh_license") return structuredClone(licenseStatus);
    if (command === "activate_license") {
      licenseStatus = createUiAuditLicenseStatus();
      return structuredClone(licenseStatus);
    }
    if (command === "start_trial") {
      licenseStatus = { ...createUiAuditLicenseStatus(), trial_active: true, trial_available: false };
      return structuredClone(licenseStatus);
    }
    if (command === "get_pending_purchase") return structuredClone(pendingPurchase);
    if (command === "create_purchase") {
      const input = args.input && typeof args.input === "object"
        ? args.input as { sku?: string; seats?: number | null }
        : undefined;
      pendingPurchase = createUiAuditPendingPurchase(input);
      return structuredClone(pendingPurchase);
    }
    if (command === "resume_purchase_checkout") {
      pendingPurchase ??= createUiAuditPendingPurchase();
      return structuredClone(pendingPurchase);
    }
    if (command === "poll_purchase_status" || command === "reconcile_purchase_status") {
      return {
        state: "checkout_pending",
        providerStatus: "created",
        amount: pendingPurchase?.amount ?? 7_900,
        currency: pendingPurchase?.currency ?? "USD",
        activated: false,
      };
    }
    if (command === "cancel_purchase_subscription") {
      return Date.parse("2026-09-01T00:00:00Z") / 1000;
    }
    if (command === "resend_purchase_license") return null;
    if (command === "forget_pending_purchase") {
      pendingPurchase = null;
      return null;
    }
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
    if (command === "usb_device_trust_score") {
      const deviceKey = String(args.deviceKey ?? "");
      const isHid = deviceKey.includes(":hid:");
      return {
        deviceKey,
        score: isHid ? 24 : 62,
        signals: {
          serialStable: !isHid,
          isHid,
          isMassStorage: !isHid,
          knownVendor: true,
          hidAlerts: isHid ? 1 : 0,
          quarantineActions: 0,
          transferBytes: isHid ? 0 : 9_437_184,
        },
      };
    }
    return uiAuditDirectResponse(command);
  }, { shouldMockEvents: true });
}
