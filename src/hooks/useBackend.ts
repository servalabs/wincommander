import { invoke } from "@tauri-apps/api/core";
import { useState, useCallback, useMemo } from "react";
import { trackBackendWork } from "../lib/activityStore";
import { clearCommand, commandId, invokeCommand } from "../lib/commandIds";

// Types for backend responses
export interface BackendResponse<T = unknown> {
  success: boolean;
  data?: T;
  error?: string;
}

export type AIControlOperationId =
  | "restore-point"
  | "package-guard"
  | "appx-packages"
  | "recall-feature"
  | "cbs-packages"
  | "ai-files"
  | "scheduled-tasks"
  | "update-cleanup"
  | "classic-photo-viewer"
  | "classic-paint"
  | "classic-snipping"
  | "classic-notepad"
  | "photos-legacy";

export interface AIControlStatus {
  isAdmin: boolean;
  classicApps: {
    photoViewer: boolean;
    paint: boolean;
    snipping: boolean;
    notepad: boolean;
    photosLegacy: boolean;
  };
}

export interface AIControlOperationResult {
  status: string;
  operation: AIControlOperationId;
  changed?: number;
  remaining?: number;
  requiresReboot?: boolean;
}

export type RoutineCleanerCategory = "system" | "browsers" | "applications" | "gaming" | "databases";

export interface RoutineCleanerItem {
  id: string;
  category: RoutineCleanerCategory;
  label: string;
  path: string;
  bytes: number;
  fileCount: number;
  recommended: boolean;
  operation: "delete" | "vacuum";
  truncated: boolean;
}

export interface RoutineCleanerScan {
  items: RoutineCleanerItem[];
  totalBytes: number;
  totalFiles: number;
  skippedTargets: number;
  cancelled: boolean;
}

export interface RoutineCleanerError {
  id: string;
  label: string;
  reason: string;
}

export interface RoutineCleanerCleanResult {
  bytesRecovered: number;
  filesCleaned: number;
  itemsCleaned: number;
  errors: RoutineCleanerError[];
  cancelled: boolean;
}

export interface DuplicateFile {
  id: string;
  name: string;
  path: string;
  size: number;
}

export interface DuplicateGroup {
  id: string;
  size: number;
  reclaimableBytes: number;
  files: DuplicateFile[];
}

export interface DuplicateScan {
  groups: DuplicateGroup[];
  scannedFiles: number;
  cancelled: boolean;
  truncated: boolean;
}

export interface DuplicateRemoveResult {
  filesRemoved: number;
  bytesRecovered: number;
  cancelled: boolean;
  errors: string[];
}

export interface EmptyFolder {
  id: string;
  name: string;
  path: string;
}

export interface EmptyFolderScan {
  folders: EmptyFolder[];
  scannedFolders: number;
  cancelled: boolean;
  truncated: boolean;
}

export interface EmptyFolderRemoveResult {
  foldersRemoved: number;
  cancelled: boolean;
  errors: string[];
}

export interface RegistryOrphan {
  id: string;
  classId: string;
  serverKind: string;
  missingServer: string;
  hive: string;
}

export interface RegistryCleanerScan {
  entries: RegistryOrphan[];
  skippedEntries: number;
}

export interface RegistryCleanerResult {
  removed: number;
  backupLocations: string[];
}

export interface ExplorerContextEntry {
  id: string;
  label: string;
  location: string;
  command: string;
  enabled: boolean;
}

export interface ExplorerContextScan {
  entries: ExplorerContextEntry[];
  skippedEntries: number;
}

export interface ExplorerContextResult {
  changed: number;
  backupLocations: string[];
}

export interface MalwareFinding {
  findingId: string;
  sha256: string | null;
  threatLabel: string;
  severity: string;
  detectionSource: string;
  canQuarantine: boolean;
}

export interface MalwareScanResult {
  scanId: string;
  scope: "quick" | "full";
  state: "running" | "completed" | "failed";
  progressPercent: number;
  findings: MalwareFinding[];
  allowlistedCount: number;
  errorCode: string | null;
}

export interface MalwareQuarantineEntry {
  quarantineId: string;
  sha256: string | null;
  threatLabel: string;
  state: "quarantined";
}

export interface SecurityThreatSnapshot {
  source: "local";
  capturedAt: string;
  defender: {
    status: "available" | "unavailable";
    realTimeEnabled?: boolean;
    antivirusEnabled?: boolean;
    recentThreatCount?: number;
    severityCounts?: Record<string, number>;
  };
  network: { interfaceCount: number; activeInterfaceCount: number };
}

export interface SecurityCveSnapshot {
  source: "osv";
  sourceTimestamp: string;
  queriedVersion: string;
  status: "ok" | "requires_approved_windows_provider";
  results: Array<{ id: string; modified: string | null }>;
  lookupPerformed: boolean;
}

export interface BrokenShortcut { id: string; name: string; path: string; target: string }
export interface ShortcutScan { shortcuts: BrokenShortcut[]; scannedShortcuts: number; cancelled: boolean; truncated: boolean }
export interface ShortcutRemoveResult { removed: number; cancelled: boolean; errors: string[] }
export interface EnvironmentFinding { id: string; scope: string; variable: string; value: string; kind: string }
export interface EnvironmentScan { entries: EnvironmentFinding[]; skippedEntries: number }
export interface EnvironmentRepairResult { repaired: number; backupLocations: string[]; errors: string[]; environmentBroadcast: boolean }
export interface UninstallLeftover { id: string; name: string; path: string; bytes: number; scope: string }
export interface UninstallLeftoverScan { entries: UninstallLeftover[]; scannedFolders: number; skippedFolders: number; cancelled: boolean; truncated: boolean }
export interface UninstallLeftoverRemoveResult { removed: number; bytesRecovered: number; cancelled: boolean; errors: string[] }
export interface ArpEntry { interface: string; address: string; physicalAddress: string; entryType: string }
export interface ArpScan { scanId: string; entries: ArpEntry[]; dynamicEntries: number }
export interface ArpClearResult { before: number; remaining: number; cleared: number }
export interface PackageUpdate { id: string; manager: string; package: string; currentVersion: string; availableVersion: string }
export interface ManagerInventory { manager: string; available: boolean; updates: PackageUpdate[]; error: string | null }
export interface PackageUpdateInventory { managers: ManagerInventory[]; cancelled: boolean }
export interface PackageUpdateResult { updated: number; cancelled: boolean; errors: string[] }
export interface FirewallRule { id: string; name: string; enabled: boolean; action: string; program: string; signed: boolean | null }
export interface FirewallAudit { rules: FirewallRule[]; cancelled: boolean; error: string | null }
export interface FirewallRemediation { changed: number; cancelled: boolean; errors: string[]; backupPath: string | null }
export interface StartupImpactEntry { id: string; name: string; source: string; location: string; command: string; executablePath: string | null; pathExists: boolean; signatureStatus: string; signer: string | null; impact: string; recommendation: string }
export interface StartupImpactScan { entries: StartupImpactEntry[]; truncated: boolean }
export interface DriverInventoryEntry { deviceName: string | null; deviceClass: string | null; deviceId: string | null; infName: string | null; manufacturer: string | null; driverVersion: string | null; driverDate: string | null; isSigned: boolean | null; signer: string | null }
export interface DriverMaintenanceInventory { drivers: DriverInventoryEntry[]; truncated: boolean; cleanupAvailable: boolean; cleanupLimitation: string }
export interface DriverUpdateSeam { provider: string; opened: boolean; limitations: string[] }

export interface InstalledBrowser {
  Name: string;
  Engine: "Chromium" | "Gecko";
  PolicyPath: string;
  InstallDir: string;
  ExePath: string;
  Hardened: boolean;
}

export interface NetworkPortRow {
  proto: "TCP" | "UDP";
  localAddr: string;
  localPort: number;
  remoteAddr: string;
  remotePort: number;
  state: string;
  pid: number;
  processName: string | null;
  processPath: string | null;
}

export interface NetworkPortsResult {
  status: "ok";
  durationMs: number;
  truncated: boolean;
  totals: { tcp: number; udp: number; shown: number };
  rows: NetworkPortRow[];
}

// ── #10 AI Security Advisor (local Ollama) ──────────────────────────────
export type LlmTask =
  | "explain-risks"
  | "suggest-hardening"
  | "summarize-logs"
  | "detect-anomalies"
  | "explain-connection";

export interface OllamaStatusResult {
  installed: boolean;
  running: boolean;
  version: string | null;
  models: string[];
  error: string | null;
}

export interface LlmAnalyzeResult {
  ok: boolean;
  task: LlmTask;
  model: string;
  markdown: string;
  tokens: number | null;
  elapsedMs: number | null;
  error: string | null;
}

// Safe Copy / Safe Paste (safe_clip.rs).
export interface SafeClipStatus {
  count: number;
  stampedAtMs: number;
}
export interface SafeSkip {
  name: string;
  reason: string;
}
export interface SafePasteResult {
  /** Absolute paths of the fresh copies to be scrubbed. */
  copied: string[];
  skipped: SafeSkip[];
  sourceCount: number;
}

// Metadata scrubber (metadata_scrubber.rs → scrub_metadata_paths). Options
// mirror the shape MetadataScrubberDialog sends; the report only types the
// fields callers here read — the interactive dialog renders the full report
// from its own local shape.
export interface ScrubMetadataOptions {
  dryRun: boolean;
  recursive: boolean;
  replaceOriginals: boolean;
  paranoid: { randomizeTimestamps: boolean; stripAltStreams: boolean };
}
export interface ScrubReportSummary {
  scrubbed: { inputPath: string; residualFields?: { label: string }[] }[];
  errors: { inputPath: string; message: string }[];
  skippedCount?: number;
  skippedFiles?: string[];
  residualCount?: number;
}

export type MacRandomizerMode = "static-random" | "rotate-on-launch";

export interface WipeDriveEntry {
  letter: string;
  label: string;
  freeGB: number;
  totalGB: number;
  mediaType: string;
  busType: string;
  isRemovable: boolean;
  isSystem: boolean;
}

export interface PhysicalNetworkAdapter {
  // Unique per binding (InterfaceGuid). Use this for Set/Restore actions.
  id: string;
  // Shared across ghost instances of the same physical card (PnPDeviceID).
  // The UI groups by this so a Wi-Fi card with 4 historical bindings shows
  // as one card with the active binding promoted and ghosts in a dropdown.
  groupId: string;
  name: string;
  description: string;
  kind: "wifi" | "ethernet" | "bluetooth";
  status: string;
  linkSpeedMbps: string | null;
  factoryMac: string | null;
  currentMac: string | null;
  isSpoofed: boolean;
}

export interface PhysicalNetworkAdaptersResult {
  status: "ok";
  adapters: PhysicalNetworkAdapter[];
}

export type DefenderExclusionSeverity = "critical" | "high" | "info";

export interface DefenderExclusionRow {
  kind: "path" | "process" | "extension" | "ip";
  value: string;
  severity: DefenderExclusionSeverity;
}

export type DefenderExclusionsResult =
  | { status: "unavailable"; error: string }
  | { status: "disabled"; reason: "service-removed" | "service-stopped"; message: string }
  | {
      status: "ok";
      total: number;
      bySeverity: Record<DefenderExclusionSeverity, number>;
      rows: DefenderExclusionRow[];
    };

export type BatteryHealthResult =
  | { status: "no-battery"; present: false }
  | { status: "unavailable"; present: true; error: string }
  | {
      status: "ok";
      present: true;
      healthPct: number | null;
      designMwh: number;
      fullChargeMwh: number;
      cycleCount: number | null;
      manufacturer: string | null;
      model: string | null;
      chemistry: string | null;
      reportedAt: string;
    };

export interface TelemetryStatus {
  serviceRunning: boolean;
  telemetryLevel: number | null;
  blocked: boolean;
}

export interface DefenderStatus {
  serviceRunning: boolean;
  realtimeEnabled: boolean;
}

export interface UpdateStatus {
  serviceRunning: boolean;
  paused: boolean;
  pausedUntil: string | null;
}

export interface ActivationStatus {
  // Windows-only. Office activation detection was dropped: OHook activates
  // at the Office process DLL layer (not SPP), so CIM/WMI/OSPP are blind
  // to it and fingerprinting MAS-OHook artefacts (scheduled tasks /
  // patcher DLLs) was too brittle to keep maintained. Diminishing returns.
  windows: {
    activated: boolean;
    edition: string | null;
  };
}

export interface HardeningStatus {
  defenderDisabled: boolean;
  updatesPaused: boolean;
  uacDisabled: boolean;
  superfetchDisabled: boolean;
  backgroundAppsDisabled: boolean;
  hibernationDisabled: boolean;
  fastStartupDisabled: boolean;
  notificationsDisabled: boolean;
  classicContextMenu: boolean;
  ntfsOptimizations: boolean;
  activityHistoryDisabled?: boolean;
  locationTrackingDisabled?: boolean;
  poshTelemetryDisabled?: boolean;
  telemetryDisabled?: boolean;
  endTaskOnTaskbar?: boolean;
  bingSearchDisabled?: boolean;
  explorerOpensThisPC?: boolean;
  fileExtensionsShown?: boolean;
  hiddenFilesShown?: boolean;
  consumerFeaturesDisabled?: boolean;
  ipv4Preferred?: boolean;
  galleryHomeRemoved?: boolean;
  detailedBSOD?: boolean;
  usbWriteProtect?: boolean;
  usbStorageLockdown?: boolean;
  advertisingIdDisabled?: boolean;
  tailoredExperiencesDisabled?: boolean;
  officeLoggingDisabled?: boolean;
  diagnosticEventTracingDisabled?: boolean;
  // Phase E — hide-recent MRU surfaces (HKCU reads)
  hideQuickAccessRecent?: boolean;
  hideQuickAccessFrequent?: boolean;
  hideRunMRU?: boolean;
  disableSearchHistory?: boolean;
  internetCommRestricted?: boolean;
  recallSnapshotsDisabled?: boolean;
  transparencyDisabled?: boolean;
  typingInsightsDisabled?: boolean;
  rdpKeepAlive?: boolean;
  rdpNoTimeouts?: boolean;
  rdpQosPriority?: boolean;
  // Host hardening (Feature 4)
  systemRestoreOff?: boolean;
  crashDumpsOff?: boolean;
  clipboardHistoryOff?: boolean;
  requirePwOnResume?: boolean;
  kernelDmaProtect?: boolean;
  // RAM-spill control (Feature 3)
  ramSpillControl?: boolean;
  // Anti-Acquisition Defenses
  acquisitionDriverBlocklist?: boolean;
  forensicToolBlock?: boolean;
  lidClosePowerOff?: boolean;
  depEnabled?: boolean;
  aslrMandatory?: boolean;
  aslrBottomUp?: boolean;
  cfgEnabled?: boolean;
  heapIntegrity?: boolean;
  sehopEnabled?: boolean;
  asrRulesEnabled?: boolean;
  controlledFolderAccessEnabled?: boolean;
  networkProtectionEnabled?: boolean;
}

export interface EncryptionStatus {
  installed: boolean;
  path: string | null;
  volumes?: Array<{
    letter: string;
    path: string | null;
    type: string;
  }>;
}

export interface BitLockerVolume {
  mountPoint: string;
  volumeType: string;          // "OperatingSystem" | "Data" | ...
  volumeStatus: string;
  encryptionMethod: string;
  protectorTypes: string[];
  recoveryPasswordPresent: boolean;
  backupUsed: boolean;
}

export interface EraseInput {
  kind: "veracrypt" | "bitlocker";
  path?: string;
  mountLetter?: string;
  mountPoint?: string;
  confirmed: boolean;
  osVolumeAck?: string;
}

export interface EraseReceipt {
  kind: string;
  label: string;
  action: string;
  status: "erased" | "erased_with_caveat" | "failed";
  verified: boolean;
  escrowWarning?: string | null;
  recoveryProtectorsRemaining?: number | null;
  keyEvicted: boolean;
  detail: string;
}

export interface CreateVolumeParams {
  Path: string;
  Size: string;
  Password: string;
  Encryption: string;
  Hash: string;
  Filesystem: string;
  Quick: boolean;
  Keyfile?: string;
  Pim?: string;
}

export interface CreateDualVolumeParams {
  Path: string;
  FirstPassword: string;
  SecondPassword: string;
  HostSize: string;
  SecondSize: string;
  Encryption: string;
  Hash: string;
  Filesystem: string;
}

export interface CreateStegoMp4Params {
  /** Carrier MP4 the container is appended to. */
  carrierMp4: string;
  /** Where to write the combined stego MP4. */
  outputPath: string;
  /** Container size, e.g. "20" or "20MB". */
  size: string;
  password: string;
}

export interface ExtractStegoMp4Params {
  /** The stego MP4 to read. */
  inputPath: string;
  /** Where to write the recovered encrypted container. */
  outputPath: string;
}

export interface VolumeInfo {
  size: string | null;
  filesystem: string | null;
  encryption: string | null;
  mode: string | null;
  readOnly: boolean;
}

export interface SystemEncryptionStatus {
  encrypted: boolean;
  progress: number | null;
  algorithm: string | null;
  mode: string | null;
}

export interface RamDisk {
  deviceNumber: number;
  letter: string;
  sizeBytes: number;
  size: string;
  type: string;
  properties: string | null;
}

export interface RamDiskStatus {
  installed: boolean;
  disks: RamDisk[];
}

export interface SystemRamInfo {
  totalBytes: number;
  totalMB: number;
  freeBytes: number;
  freeMB: number;
}

export interface CreateRamDiskParams {
  SizeMB: number;
  DriveLetter: string;
  Filesystem: string;
  Label: string;
  ReadOnly: boolean;
  Quick: boolean;
}

export interface EncryptionPartition {
  diskNumber: number;
  partitionNumber: number;
  driveLetter: string | null;
  size: string;
  sizeBytes: number;
  devicePath: string;
  busType: string;
  model: string;
}

function parseSizeToMB(size: string): number {
  const raw = size.trim().toUpperCase();
  const match = raw.match(/^(\d+(?:\.\d+)?)([MGT])$/);
  if (!match) return 0;

  const value = Number(match[1]);
  const unit = match[2];
  if (!Number.isFinite(value) || value <= 0) return 0;

  switch (unit) {
    case "M":
      return Math.round(value);
    case "G":
      return Math.round(value * 1024);
    case "T":
      return Math.round(value * 1024 * 1024);
    default:
      return 0;
  }
}

export interface EventLogEntry {
  name: string;
  count: number;
  oldest: string | null;
  newest: string | null;
  sizeMb: number | null;
}

export interface SRUMEntry {
  name: string;
  pid: number;
  path: string;
  owner: string;
  cpuTime: number;
  memoryKB: number;
  threadCount: number;
}

export interface PSHistoryEntry {
  id: number;
  command: string;
}

export interface RecentFileEntry {
  name: string;
  extension: string;
  target: string | null;
  lastModified: string;
  sizeBytes: number;
}

export interface RecycleBinEntry {
  originalPath: string;
  deletedTime: string;
  sizeBytes: number;
  sizeKB: number;
  account: string;
  sid: string;
  drive: string;
}

export interface RDPHistoryEntry {
  type: 'MRU' | 'Server' | 'SavedCredential' | 'DefaultRDP';
  host: string;
  username?: string | null;
  key: string;
  lastModified?: string | null;
}

export interface ConnectivityHistoryEntry {
  source: string;
  type: 'Profile' | 'Signature';
  name: string;
  key: string;
  description?: string | null;
  dnsSuffix?: string | null;
  category?: number | null;
}

export interface JumpListEntry {
  name: string;
  type: 'Automatic' | 'Custom';
  sizeKB: number;
  lastModified: string;
}

export interface BrowserArtifact {
  name: string;
  sizeKB: number;
}

export interface BrowserFootprintBrowser {
  browser: string;
  profilePath: string;
  artifacts: BrowserArtifact[];
  totalSizeKB: number;
}

export interface PrefetchEntry {
  name: string;
  fileName: string;
  sizeKB: number;
  lastRun: string;
  created: string;
}

export interface ShadowCopy {
  id: string;
  drive: string;
  deviceObject: string;
  created: string;
  originatingMachine: string;
  serviceMachine: string;
  clientAccessible: boolean;
  persistent: boolean;
  stateStr: string;
}

export interface NTFSJournal {
  drive: string;
  present: boolean;
  journalId: string | null;
  maxSize: string | null;
}

export interface FirewallStatus {

  profiles: Array<{
    Name: string;
    Enabled: boolean;
    DefaultOutboundAction: string;
  }>;
}

export interface AppBranding {
  companyName: string;
  productName: string;
}

export interface AppLicenseStatus {
  configured: boolean;
  licensed: boolean;
  valid: boolean;
  reason?: string | null;
  plan?: string | null;
  features: string[];
  base_features?: string[];
  active_service_features?: string[];
  entitlement_expires_at?: number | null;
  service_expires_at?: number | null;
  expires_at?: number | null;
  last_verified_at?: number | null;
  grace_until?: number | null;
  device_hash: string;
  seats_used?: number | null;
  seat_limit?: number | null;
  /** True when the app is running as Windows To Go (portable USB OS). */
  is_portable?: boolean;
  /** Whether a free trial is currently active. */
  trial_active?: boolean;
  /** Unix timestamp when the trial expires. */
  trial_expires_at?: number | null;
  /** True when no trial has ever been started on this device. */
  trial_available?: boolean;
  /** True when a trial was started but has since expired. Features still work (soft expiry). */
  trial_expired?: boolean;
}



export interface SystemInfo {
  osName: string;
  osVersion: string;
  buildNumber: string;
  hostname: string;
  deviceType?: "Laptop" | "Desktop" | "Unknown";
  isAdmin: boolean;
  cpu: string;
  cpuUsage: number;
  cpuTemp: number;
  ram: string;
  ramUsage: number;
  ramUsedGb?: number;
  ramTotalGb?: number;
  gpu: string;
  disks: Array<{
    id: string;
    label: string;
    totalGb: number;
    usedGb: number;
    freeGb: number;
    percent: number;
    healthPercent?: number | null;
  }>;
  uptime: {
    days: number;
    hours: number;
    minutes: number;
  };
}

export interface DriveSmartHealthEntry {
  driveLetter: string;
  healthPercent: number | null;
  passed: boolean | null;
  source: string;
}

export interface DriveSmartHealthResult {
  smartctlAvailable: boolean;
  drives: DriveSmartHealthEntry[];
}

export interface StorageStatEntry {
  count: number;
  sizeGb: number;
  source?: 'everything' | 'scan';
}

export interface StorageStats {
  documents: StorageStatEntry;
  pdfs: StorageStatEntry;
  spreadsheets: StorageStatEntry;
  presentations: StorageStatEntry;
  images: StorageStatEntry;
  videos: StorageStatEntry;
  audio: StorageStatEntry;
  archives: StorageStatEntry;
  code: StorageStatEntry;
}

export interface AppManifest {
  apps: Array<{
    id: string;
    name: string;
    description: string;
    category: string;
  }>;
}

export interface BlocklistStatus {
  applied: string[];
  available: Array<{
    name: string;
    entries: number;
  }>;
}

export interface PrivacyStatus {
  pagefile: boolean;
  prefetch: boolean;
  recentFiles: boolean;
  jumpLists: boolean;
  thumbnailCache: boolean;
}

export interface AppPrivacyCapabilitiesStatus {
  webcam: boolean;
  microphone: boolean;
  contacts: boolean;
  appointments: boolean;
  phoneCall: boolean;
  phoneCallHistory: boolean;
  chat: boolean;
  userNotificationListener: boolean;
  documentsLibrary: boolean;
  picturesLibrary: boolean;
  videosLibrary: boolean;
  broadFileSystemAccess: boolean;
  gazeInput: boolean;
  appDiagnostics: boolean;
  userAccountInformation: boolean;
  bluetoothSync: boolean;
}

export interface DNSStatus {
  servers: string[];
  adapter: string | null;
  provider: string | null;
  resolverIp?: string | null;
  resolverOrg?: string | null;
  dohTemplate?: string | null;
  dohId?: string | null;
  deviceName?: string | null;
}

export interface UsbHistoryItem {
  deviceId: string;
  friendlyName?: string | null;
  manufacturer?: string | null;
  className?: string | null;
}

export interface DnsCacheEntry {
  name: string;
  data: string;
  recordType?: string | null;
  status?: string | null;
  ttl?: number | null;
}

export interface ExecutionCacheEntry {
  path: string;
  source: string;
}

export interface ShellBagEntry {
  path: string;
  lastModified?: string;
  source?: string;
}

export interface ProcessIntelligenceEntry {
  name: string;
  pid: number;
  path: string;
  signed: string;   // Signature status: Valid, NotSigned, UnknownError, etc.
  signer?: string | null;  // Certificate subject when Valid
  elevated: string;
}

export interface WlanProfile {
  name: string;
  password?: string | null;
}

export interface BluetoothDevice {
  id: string;
  lastSeen?: string;
  name?: string;
}

export interface NetworkDrive {
  Name: string;
  DisplayRoot: string;
}

export interface MeshVPNPeer {
  ID: string;
  Hostname: string;
  OS: string;
  Online: boolean;
  Active: boolean;
  Relay: string;
  LastSeen: string;
  IPs: string[];
  RxBytes: number;
  TxBytes: number;
  CurAddr: string;
  ExitNodeOption: boolean;
}

export interface MeshVPNStatus {
  installed: boolean;
  running?: boolean;
  backendState?: string;
  loggedOut?: boolean;
  authUrl?: string;
  health?: string[];
  self?: {
    Hostname: string;
    Online: boolean;
    Active: boolean;
    IPs?: string[];
    ExitNode?: boolean;
    ExitNodeOption?: boolean;
  };
  prefs?: {
    AdvertiseExitNode: boolean;
    ExitNodeIP: string;
    ExitNodeAllowLANAccess: boolean;
    ShieldsUp: boolean;
    Unattended: boolean;
    AcceptRoutes: boolean;
    AcceptDNS: boolean;
  };
  MagicDNSSuffix?: string;
  peers?: MeshVPNPeer[];
  error?: string;
}

export interface StartupStatus {
  systemInfo: SystemInfo;
  hardeningStatus: HardeningStatus;
  telemetryStatus: TelemetryStatus;
  privacyProtection: {
    enabled: boolean;
    pagefile: boolean;
    prefetch: boolean;
    recentFiles: boolean;
    jumpLists: boolean;
    thumbnailCache: boolean;
  };
  // clipboardSchedule + rdpSchedule status fields removed alongside the
  // per-toggle Enable-/Disable-/Get-*Schedule commands. The per-card
  // scheduler exposes its state via getAutoEraseSchedules() instead.
  systemError?: string;

  // Extended Privacy
  clipboardHistory?: { clipboardHistoryDisabled: boolean; cloudClipboardDisabled: boolean };
  windowsSuggestions?: { disabled: boolean };
  appPrivacyCapabilities?: AppPrivacyCapabilitiesStatus;
  officePrivacy?: { disabled: boolean };
  lockScreenPrivacy?: { disabled: boolean };
  setupNags?: { disabled: boolean };
  productivity?: { running: boolean; details: { server: boolean; input: boolean; active: boolean } };
}

export interface StartupItem {
  Name: string;
  Command: string;
  RamUsageMB: number;
  Status: "Running" | "Stopped" | "Disabled";
  IsEnabled: boolean;
  Source: "Registry" | "Folder" | "Backup";
  Location: string;
  IsSafe: boolean;
  Recommendation: "Keep" | "Disable" | "Neutral";
  Category: string;
  Description: string;
}



const getErrorMessage = (result: unknown) => {
  if (result && typeof result === "object" && ("message" in result || "debugInfo" in result)) {
    const resValue = result as { message?: string; debugInfo?: string; details?: string; command?: string };
    const message = String(resValue.message || "Command failed");
    const details = resValue.details;
    const command = resValue.command;
    const debugInfo = resValue.debugInfo;
    const parts = [message];
    if (command) {
      parts.push(`Command: ${command}`);
    }
    if (details) {
      parts.push(details);
    }
    if (debugInfo) {
      parts.push(`\n--- Diagnostic Logs ---\n${debugInfo}`);
    }
    return parts.join("\n");
  }
  if (result && typeof result === "object" && "error" in result) {
    return String((result as { error?: string }).error || "Command failed");
  }
  if (result && typeof result === "object" && "ok" in result && (result as { ok?: boolean }).ok === false) {
    const r = result as { stdout?: string; message?: string };
    return String(r.message || r.stdout || "Command failed");
  }
  return "Command failed";
};

// ── BCU (Bulk Crap Uninstaller) Types ──────────────────────────────────────

export interface BcuApplication {
  displayName: string;
  publisher: string;
  displayVersion: string;
  installDate: string;
  installLocation: string;
  installSource: string;
  uninstallString: string;
  quietUninstall: string;
  estimatedSizeKB: number;
  isProtected: boolean;
  isSystemComponent: boolean;
  isOrphaned: boolean;
  isUpdate: boolean;
  isValid: boolean;
  isRegistered: boolean;
  isWebBrowser: boolean;
  canQuietUninstall: boolean;
  uninstallerKind: string;   // Chocolatey|InnoSetup|InstallShield|Msiexec|Nsis|PowerShell|SimpleDelete|StoreApp|Unknown|WindowsFeature
  is64Bit: string;           // "X64" | "X86" | "Unknown"
  registryKeyName: string;
  registryPath: string;
  aboutUrl: string;
  comment: string;
  displayIcon: string;
}

export interface BcuApplicationListResult {
  apps: BcuApplication[];
  totalCount: number;
  scanTime: string;
}

export interface BcuOrphanedFolder {
  path: string;
  name: string;
  isEmpty: boolean;
  isOrphaned: boolean;
  sizeBytes: number;
}

export interface BcuProgramFilesCleanupResult {
  items: BcuOrphanedFolder[];
  totalCount: number;
  totalSize: number;
}

export interface BcuWindowsFeature {
  featureName: string;
  displayName: string;
  state: string;
  restartRequired: boolean;
}

export interface BcuWindowsFeaturesResult {
  features: BcuWindowsFeature[];
  totalCount: number;
}

export interface BcuStartupItem {
  name: string;
  command: string;
  location: string;
  type: string;
  enabled: boolean;
}

export interface BcuStartupItemsResult {
  items: BcuStartupItem[];
  totalCount: number;
}

export interface BcuRegistryLeftover {
  displayName: string;
  registryPath: string;
  registryKey: string;
  installLocation: string;
  uninstallString: string;
  publisher: string;
  hive: string;
}

export interface BcuRegistryLeftoversResult {
  leftovers: BcuRegistryLeftover[];
  totalCount: number;
}

const getUnknownError = (err: unknown) => {
  if (err instanceof Error) {
    return err.message;
  }
  try {
    const serialized = JSON.stringify(err);
    return serialized && serialized !== "{}" ? serialized : String(err);
  } catch {
    return String(err);
  }
};

function serializeBackendParams(
  params: Record<string, string | number | boolean> = {}
): Record<string, string> {
  const serializedParams: Record<string, string> = {};
  Object.entries(params).forEach(([k, v]) => {
    if (typeof v === "boolean") serializedParams[k] = v ? "true" : "false";
    else if (typeof v === "number") serializedParams[k] = String(v);
    else if (v == null) serializedParams[k] = "";
    else serializedParams[k] = String(v);
  });
  return serializedParams;
}

function getBackendRequestKey(command: string, params: Record<string, string>): string {
  const orderedParams = Object.keys(params)
    .sort()
    .reduce<Record<string, string>>((acc, key) => {
      acc[key] = params[key];
      return acc;
    }, {});
  return `${command}:${JSON.stringify(orderedParams)}`;
}

const inFlightBackendRequests = new Map<string, Promise<unknown>>();

async function hasActivePaidEntitlement(): Promise<boolean> {
  try {
    const status = await invoke<AppLicenseStatus>("get_license_status");
    return (status.licensed === true && status.valid === true) || status.trial_active === true;
  } catch {
    return false;
  }
}

async function normalizeBackendError(err: unknown): Promise<string> {
  const message = getUnknownError(err) || "Unknown error";
  if (message.startsWith("PRO_NOT_INSTALLED:")) {
    const detail = message.slice("PRO_NOT_INSTALLED:".length).trim();
    if (await hasActivePaidEntitlement()) {
      window.dispatchEvent(new CustomEvent("pro-install-open"));
      return detail;
    }
    return "WinCommander Pro entitlement required. Activate a license or start the 16-day free trial.";
  }
  // Paid background work can fail while entitlement state is still resolving
  // or after a service entitlement lapses. A backend error is not an explicit
  // request to buy, so it must never repeatedly reopen the global paywall.
  // Interactive locked controls dispatch `license-gate-open` themselves.
  return message;
}

async function runBackendScriptTracked<T>(
  command: string,
  params: Record<string, string | number | boolean> = {}
): Promise<T> {
  const serializedParams = serializeBackendParams(params);
  const requestKey = getBackendRequestKey(command, serializedParams);
  const existingRequest = inFlightBackendRequests.get(requestKey) as Promise<T> | undefined;
  if (existingRequest) {
    return existingRequest;
  }

  const request = trackBackendWork(
    invoke<T>("run_backend_script", {
      command,
      params: serializedParams,
    })
  );

  inFlightBackendRequests.set(requestKey, request as Promise<unknown>);
  try {
    return await request;
  } finally {
    inFlightBackendRequests.delete(requestKey);
  }
}

// Hook for executing backend commands
export function useBackend() {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const execute = useCallback(async <T>(
    command: string,
    params: Record<string, string | boolean | number> = {}
  ): Promise<BackendResponse<T>> => {
    setLoading(true);
    setError(null);

    try {
      // KT: De-dupe identical in-flight backend commands so a double-click
      // cannot launch the same PowerShell task twice before the UI disables.
      const result = await runBackendScriptTracked<any>(command, params);

      // Check for error in response object (from PowerShell logic), or an
      // explicit ok:false from a Pro handler ({"ok": false, "stdout": "fail: ..."})
      // — Pro responses carry no `error` key at all, so without this check
      // a real Pro-side failure would fall through to the success return below.
      const isPoshError = result && typeof result === "object" && "error" in result && (result as { error?: boolean }).error;
      const isProOkFalse = result && typeof result === "object" && "ok" in result && (result as { ok?: boolean }).ok === false;
      if (isPoshError || isProOkFalse) {
        const errorMsg = getErrorMessage(result);
        setError(errorMsg);
        return { success: false, error: errorMsg };
      }

      // If data property exists (some commands might wrap it, but usually top level is the data)
      // Actually backend.ps1 returns the data directly as the object.
      // So 'result' IS the data.
      return { success: true, data: result as T };

    } catch (err) {
      const errorMsg = await normalizeBackendError(err);
      setError(errorMsg);
      return { success: false, error: errorMsg };
    } finally {
      setLoading(false);
    }
  }, []);

  // Convenience methods for common operations
  const backend = useMemo(() => ({
    // Power Plan
    setPowerPlan: (mode: 'balanced' | 'powersaving' | 'performance' | 'ultimate') =>
      execute("Set-PowerPlan", { Mode: mode }),

    // System Info
    getSystemInfo: () => execute<SystemInfo>("Get-SystemInfo"),
    getDriveSmartHealth: () => invoke<DriveSmartHealthResult>("get_drive_smart_health"),
    getWipeDriveList: () => invoke<WipeDriveEntry[]>("get_wipe_drive_list"),
    getStorageStats: () => execute<StorageStats>("Get-StorageStats"),
    getStartupStatus: () => execute<StartupStatus>("Get-StartupStatus"),
    getAIControlStatus: () => execute<AIControlStatus>("Get-AIControlStatus"),
    runAIControlOperation: (operation: AIControlOperationId, mode: "apply" | "revert" = "apply", backup = true) =>
      execute<AIControlOperationResult>("Invoke-AIControlOperation", { Operation: operation, Mode: mode, Backup: backup }),

    // Telemetry
    getTelemetryStatus: () => execute<TelemetryStatus>("Get-TelemetryStatus"),
    disableTelemetry: () => execute("Disable-Telemetry"),
    enableTelemetry: () => execute("Enable-Telemetry"),
    disableCopilot: () => execute("Disable-Copilot"),
    enableCopilot: () => execute("Enable-Copilot"),

    // Windows Defender
    getDefenderStatus: () => execute<DefenderStatus>("Get-DefenderStatus"),
    disableDefender: () => execute(commandId("Disable-", "Windows", "Defender")),
    enableDefender: () => execute("Enable-WindowsDefender"),

    // Windows Update
    getUpdateStatus: () => execute<UpdateStatus>("Get-UpdateStatus"),
    disableUpdates: () => execute("Disable-WindowsUpdate"),
    enableUpdates: () => execute("Enable-WindowsUpdate"),

    // Hardening status
    getHardeningStatus: () => execute<HardeningStatus>("Get-HardeningStatus"),

    // Hibernation & Fast Startup
    disableHibernation: () => execute("Disable-Hibernation"),
    enableHibernation: () => execute("Enable-Hibernation"),
    disableFastStartup: () => execute("Disable-FastStartup"),
    enableFastStartup: () => execute("Enable-FastStartup"),

    // UAC
    disableUAC: () => execute("Disable-UAC"),
    enableUAC: () => execute("Enable-UAC"),

    // Superfetch
    disableSuperfetch: () => execute("Disable-Superfetch"),
    enableSuperfetch: () => execute("Enable-Superfetch"),

    // Background Apps
    disableBackgroundApps: () => execute("Disable-BackgroundApps"),
    enableBackgroundApps: () => execute("Enable-BackgroundApps"),

    // Notifications
    disableNotifications: () => execute("Disable-Notifications"),
    enableNotifications: () => execute("Enable-Notifications"),

    // Classic Context Menu
    enableClassicContextMenu: () => execute("Enable-ClassicContextMenu"),
    disableClassicContextMenu: () => execute("Disable-ClassicContextMenu"),

    // Explorer Clutter
    enableRemoveGalleryHome: () => execute("Enable-RemoveGalleryHome"),
    disableRemoveGalleryHome: () => execute("Disable-RemoveGalleryHome"),

    // NTFS Optimizations
    enableNTFSOptimizations: () => execute("Enable-NTFSOptimizations"),
    disableNTFSOptimizations: () => execute("Disable-NTFSOptimizations"),

    // Consumer Features
    disableConsumerFeatures: () => execute("Disable-ConsumerFeatures"),
    enableConsumerFeatures: () => execute("Enable-ConsumerFeatures"),

    // Privacy Protection
    clearClipboard: () => execute(clearCommand("Clipboard")),
    clearPowerShellHistory: () => execute("Clear-PowerShellHistory"),
    clearRecentFiles: () => execute(clearCommand("RecentFiles")),
    clearRDPHistory: () => execute(clearCommand("RDPHistory")),
    clearRDPPasswords: () => execute(clearCommand("RDPPasswords")),
    clearJumpLists: () => execute(clearCommand("JumpLists")),
    clearBrowserFootprints: (browser?: string, profilePath?: string) => {
        const params: Record<string, string> = {};
        if (browser) params.Browser = browser;
        if (profilePath) params.ProfilePath = profilePath;
        return execute(clearCommand("BrowserFootprints"), params);
    },
    clearPrefetch: () => execute(clearCommand("Prefetch")),
    clearShadowCopies: () => execute(clearCommand("ShadowCopies")),
    getUsbHistory: () => execute<{ devices: UsbHistoryItem[] }>("Get-USBDeviceHistory"),
    clearUsbHistory: () => execute(clearCommand("USBDeviceHistory")),
    getDnsCacheEntries: () => execute<{ entries: DnsCacheEntry[] }>("Get-DnsCacheEntries"),
    flushDnsCache: () => execute("Clear-DnsCache"),
    getExecutionCache: () => execute<{ entries: ExecutionCacheEntry[] }>("Get-ExecutionCache"),
    clearExecutionCache: () => execute(clearCommand("ExecutionCache")),
    invokeDiskCleanup: () => execute("Invoke-DiskCleanup"),
    invokeMasterPrivacyClean: () => execute(invokeCommand("MasterPrivacyClean")),
    getWlanProfiles: () => execute<{ profiles: WlanProfile[] }>("Get-WlanProfiles"),
    removeWlanProfile: (name: string) => execute(commandId("Remove-", "Wlan", "Profile"), { Name: name }),
    getBluetoothDevices: () => execute<{ devices: BluetoothDevice[] }>("Get-BluetoothDevices"),
    clearBluetoothHistory: () => execute(clearCommand("BluetoothDevices")),
    getNetworkDrives: () => execute<{ drives: NetworkDrive[] }>("Get-NetworkDrives"),
    clearNetworkDrives: () => execute(clearCommand("NetworkDrives")),

    // Scheduled Clipboard Erase — REMOVED, replaced by setAutoEraseSchedule('clipboard', n)
    disableClipboardHistory: () => execute("Disable-ClipboardHistory"),
    enableClipboardHistory: () => execute("Enable-ClipboardHistory"),
    getClipboardHistoryStatus: () => execute<{ clipboardHistoryDisabled: boolean; cloudClipboardDisabled: boolean; historyItems?: Array<{ type: string; preview: string; charCount: number }> }>("Get-ClipboardHistoryStatus"),
    disableCloudClipboardSync: () => execute("Disable-CloudClipboardSync"),
    enableCloudClipboardSync: () => execute("Enable-CloudClipboardSync"),

    // Scheduled RDP Erase — REMOVED, replaced by setAutoEraseSchedule('rdpHistory', n)

    // Unified per-card auto-erase scheduler (Privacy Clean panel).
    // categoryId matches the CleanupCategory.id from cleanupCategories.ts.
    // intervalMinutes is clamped server-side to >= 1; runAsSystem switches
    // the scheduled task principal to NT AUTHORITY\SYSTEM (required for
    // Security event log, BT pairings, etc).
    setAutoEraseSchedule: (categoryId: string, intervalMinutes: number, runAsSystem: boolean = false) =>
      execute<{ status: string; categoryId: string; taskName: string; intervalMinutes: number; runAsSystem: boolean }>(
        "Set-AutoEraseSchedule",
        { CategoryId: categoryId, IntervalMinutes: intervalMinutes, RunAsSystem: runAsSystem },
      ),
    removeAutoEraseSchedule: (categoryId: string) =>
      execute<{ status: string; categoryId: string }>(
        "Remove-AutoEraseSchedule",
        { CategoryId: categoryId },
      ),
    getAutoEraseSchedules: () =>
      execute<{
        schedules: Array<{
          categoryId: string;
          taskName: string;
          enabled: boolean;
          intervalMinutes: number;
          targetUser: string | null;
          lastRun: string | null;
          nextRun: string | null;
          lastResult: number | null;
        }>;
        total: number;
      }>("Get-AutoEraseSchedules"),
    getAutoEraseSupportedCategories: () =>
      execute<{ categories: string[] }>("Get-AutoEraseSupportedCategories"),
    invokeAutoEraseMigration: () =>
      execute<{ status: string; migrated: string[] }>("Invoke-AutoEraseMigration"),

    // ── Multi-user scope ────────────────────────────────────────────────────
    // All four run FREE / in-process in the Free binary — their PowerShell
    // lives in privacy/cleanup and there is no Pro sidecar handler for them.
    // The real privilege boundary (reading/clearing other users' hives) is
    // Windows Administrator, enforced server-side via Test-IsAdmin.
    getUserProfiles: () =>
      execute<{
        profiles: Array<{ name: string; displayName?: string; path: string; sid?: string; isCurrent?: boolean }>;
        total: number;
        currentUser: string;
        currentSid?: string;
        isAdmin: boolean;
      }>("Get-UserProfiles"),
    // Scan cleanup traces for users. The UI views one user at a time, so it
    // passes a single targetUsers entry; omit it to scan every account.
    getCleanupSummaryAllUsers: (categoryIds?: string[], targetUsers?: string[]) =>
      execute<{
        status: string;
        userCount: number;
        isAdmin: boolean;
        users: Array<{
          username: string;
          path: string;
          sid: string;
          isCurrentUser: boolean;
          isLoggedIn: boolean;
          total: number;
          hiveAvailable: boolean;
          hiveError?: string;
          categories: Array<{ id: string; count: number; items: string[] }>;
        }>;
      }>("Get-CleanupSummaryAllUsers", {
        ...(categoryIds && categoryIds.length > 0 ? { CategoryIds: categoryIds.join(',') } : {}),
        ...(targetUsers && targetUsers.length > 0 ? { TargetUsers: targetUsers.join(',') } : {}),
      }),
    getLoggedInUsers: () =>
      execute<{ users: string[] }>("Get-LoggedInUsers"),
    invokeCleanupClearAllUsers: (
      categoryIds: string[],
      targetUsers?: string[],
    ) =>
      execute<{
        status: string;
        cleaned: number;
        partial: number;
        results: Array<{
          user: string;
          status: 'cleaned' | 'partial';
          note?: string;
          skippedCats?: string[];
          cleanedCats?: string[];
        }>;
      }>("Invoke-CleanupClearAllUsers", {
        CategoryIds: categoryIds.join(','),
        TargetUsers: (targetUsers ?? []).join(','),
      }),
    setMultiUserAutoEraseSchedule: (
      categoryId: string,
      intervalMinutes: number,
      targetUsers: string[],
      runAsSystem = false,
    ) =>
      execute<{ status: string; results: unknown[]; total: number }>(
        "Set-MultiUserAutoEraseSchedule",
        {
          CategoryId: categoryId,
          IntervalMinutes: intervalMinutes,
          RunAsSystem: runAsSystem,
          TargetUsers: targetUsers.join(','),
        },
      ),
    removeMultiUserAutoEraseSchedule: (
      categoryId: string,
      targetUsers?: string[],
    ) =>
      execute<{ status: string; categoryId: string; removed: string[] }>(
        "Remove-MultiUserAutoEraseSchedule",
        {
          CategoryId: categoryId,
          TargetUsers: (targetUsers ?? []).join(','),
        },
      ),

    // Privacy Protection Mode
    enablePrivacyProtection: () => execute("Enable-PrivacyProtection"),
    disablePrivacyProtection: () => execute("Disable-PrivacyProtection"),
    getPrivacyProtectionStatus: () => execute<{ enabled: boolean; pagefile: boolean; prefetch: boolean; recentFiles: boolean; jumpLists: boolean; thumbnailCache: boolean }>("Get-PrivacyProtectionStatus"),

    // Individual Privacy Toggles
    disablePagefile: () => execute("Disable-Pagefile"),
    enablePagefile: () => execute("Enable-Pagefile"),
    disablePrefetch: () => execute("Disable-Prefetch"),
    enablePrefetch: () => execute("Enable-Prefetch"),
    disableRecentFilesTracking: () => execute("Disable-RecentFilesTracking"),
    enableRecentFilesTracking: () => execute("Enable-RecentFilesTracking"),
    disableJumpLists: () => execute("Disable-JumpLists"),
    enableJumpLists: () => execute("Enable-JumpLists"),
    disableThumbnailCache: () => execute("Disable-ThumbnailCache"),
    enableThumbnailCache: () => execute("Enable-ThumbnailCache"),

    // System Cleanup — deep traces
    getShellBags: () => execute<{ entries: ShellBagEntry[] }>("Get-ShellBags"),
    clearShellBags: () => execute(clearCommand("ShellBags")),
    clearSRUM: () => execute(clearCommand("SRUM")),
    getSRUMData: () => execute<{ entries: SRUMEntry[]; srumSizeMb: number | null; total: number }>("Get-SRUMData"),
    clearEventLogs: () => execute(clearCommand("EventLogs")),
    getEventLogSummary: () => execute<{ logs: EventLogEntry[]; total: number }>("Get-EventLogSummary"),
    getPSHistory: () => execute<{ entries: PSHistoryEntry[]; historyPath: string | null; total: number; fileTotal: number }>("Get-PSHistory"),
    clearNTFSJournals: () => execute(clearCommand("NTFSJournals")),

    // System Cleanup — trace viewers
    getRecentFiles: () => execute<{ entries: RecentFileEntry[]; total: number; path: string }>("Get-RecentFiles"),
    getRDPHistory: () => execute<{ entries: RDPHistoryEntry[]; total: number }>("Get-RDPHistory"),
    getConnectivityHistory: () => execute<{ entries: ConnectivityHistoryEntry[]; total: number }>("Get-ConnectivityHistory"),
    getJumpLists: () => execute<{ entries: JumpListEntry[]; total: number }>("Get-JumpLists"),
    getBrowserFootprints: () => execute<{ browsers: BrowserFootprintBrowser[]; totalBrowsers: number }>("Get-BrowserFootprints"),
    getPrefetchFiles: () => execute<{ entries: PrefetchEntry[]; total: number; path: string; accessDenied: boolean; enablePrefetcher: number }>("Get-PrefetchFiles"),
    getShadowCopies: () => execute<{ copies: ShadowCopy[]; total: number; vssRunning: boolean }>("Get-ShadowCopies"),
    getNTFSJournals: () => execute<{ journals: NTFSJournal[]; total: number }>("Get-NTFSJournals"),

    // GROUP I-A: Advanced DFIR — viewers
    getAmcacheEntries: () => execute<{ entries: Array<{ category: string; count: number; sample: Array<{ id: string; name: string; path: string }> }>; hveFileSizeMb: number; hveFileExists: boolean; total: number }>("Get-AmcacheEntries"),
    getNTUserTraces: () => execute<{ sections: Array<{ name: string; count: number; entries: Array<{ key: string; value: string }> }>; total: number }>("Get-NTUserTraces"),
    getNotepadStateFiles: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; packageFound: boolean; totalSizeKB: number }>("Get-NotepadStateFiles"),
    getPCAInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string; type: string }>; total: number; totalSizeMB: number; pcaSvcState: string }>("Get-PCAInfo"),
    getCrashDumpList: () => execute<{ dumps: Array<{ source: string; name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-CrashDumpList"),
    getSQLiteWALList: () => execute<{ files: Array<{ name: string; sizeKB: number; dir: string; modified: string }>; total: number; totalSizeMB: number }>("Get-SQLiteWALList"),
    getRecallDatabaseInfo: () => execute<{ databases: Array<{ source: string; name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-RecallDatabaseInfo"),
    getSearchIndexInfo: () => execute<{ files: Array<{ label: string; name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number; wsearchState: string }>("Get-SearchIndexInfo"),
    getPrintSpoolerInfo: () => execute<{ files: Array<{ source: string; name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number; spoolerState: string }>("Get-PrintSpoolerInfo"),
    getRecycleBinInfo: () => execute<{ items: RecycleBinEntry[]; total: number; totalSizeKB: number }>("Get-RecycleBinInfo"),
    getWebCacheInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-WebCacheInfo"),
    getThumbnailCacheInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-ThumbnailCacheInfo"),
    getNotificationDbInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-NotificationDatabaseInfo"),
    getBranchCacheInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-BranchCacheInfo"),
    getEventTranscriptInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-EventTranscriptInfo"),
    getActivitiesTimelineInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-ActivitiesTimelineInfo"),
    getRdpBitmapCacheInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-RdpBitmapCacheInfo"),
    getServicingLogsInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-ServicingLogsInfo"),
    getDeviceInstallLogsInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-DeviceInstallLogsInfo"),
    getUsageTraceLogsInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-UsageTraceLogsInfo"),
    getDefenderHistoryInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-DefenderHistoryInfo"),
    getWSLDataInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-WSLDataInfo"),
    getDockerDesktopDataInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-DockerDesktopDataInfo"),
    getVirtualMachineArtifactsInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-VirtualMachineArtifactsInfo"),
    getDeveloperCachesInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-DeveloperCachesInfo"),
    getCredentialManagerInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-CredentialManagerInfo"),
    getNetworkWizardHistoryInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-NetworkWizardHistoryInfo"),
    getWERHistoryInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-WERHistoryInfo"),
    getInactiveUserProtectionMetadataInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-InactiveUserProtectionMetadataInfo"),
    getStickyNotesInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-StickyNotesInfo"),
    getOneDriveMetadataInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-OneDriveMetadataInfo"),
    getSpotlightCacheInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-SpotlightCacheInfo"),
    getFontCacheInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-FontCacheInfo"),
    getLegacyIconCacheInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-LegacyIconCacheInfo"),
    getGameCapturesInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-GameCapturesInfo"),
    getPhotosCacheInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-PhotosCacheInfo"),
    getXboxCacheInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-XboxCacheInfo"),
    getCommunicationCachesInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-CommunicationCachesInfo"),
    getEditorHistoryInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-EditorHistoryInfo"),
    getGitActivityInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-GitActivityInfo"),
    getSSHStateInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-SSHStateInfo"),
    getRemoteAccessLogsInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-RemoteAccessLogsInfo"),
    getPasswordManagerCachesInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-PasswordManagerCachesInfo"),
    getGameLauncherLogsInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-GameLauncherLogsInfo"),
    getAdobeRecentInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-AdobeRecentInfo"),
    getOfficeTempFilesInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-OfficeTempFilesInfo"),
    getFirewallLogInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-FirewallLogInfo"),
    getNeighborCacheInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-NeighborCacheInfo"),
    getNetBIOSCacheInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-NetBIOSCacheInfo"),
    getGeolocationCacheInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-GeolocationCacheInfo"),
    getVPNPhonebooksInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-VPNPhonebooksInfo"),
    getProxyCacheInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-ProxyCacheInfo"),
    getCloudPlaceholdersInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-CloudPlaceholdersInfo"),
    getBITSQueueInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-BITSQueueInfo"),
    getCellularHistoryInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-CellularHistoryInfo"),
    getAppLaunchHistoryInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-AppLaunchHistoryInfo"),
    getOfficeMruInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-OfficeMruInfo"),
    getEmbeddedWebCacheInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-EmbeddedWebCacheInfo"),
    getP2PUpdateCacheInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-P2PUpdateCacheInfo"),
    getReliabilityHistoryInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-ReliabilityHistoryInfo"),
    getExplorerSearchHistoryInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-ExplorerSearchHistoryInfo"),
    getSearchPersonalizationInfo: () => execute<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number }>("Get-SearchPersonalizationInfo"),
    // GROUP I-A: Advanced DFIR — erases
    clearAmcache: () => execute<{ status: string; clearedKeys: number; bootPurgeScheduled: boolean }>(clearCommand("Amcache")),
    clearRecycleBinMetadata: () => execute<{ ok: boolean; stdout: string }>(clearCommand("RecycleBinMetadata")),
    clearNTUserTraces: () => execute<{ status: string; removedEntries: number }>(clearCommand("NTUserTraces")),
    clearConnectivityHistory: () => execute(clearCommand("ConnectivityHistory")),
    clearNotepadState: () => execute<{ status: string; removedFiles: number; packaged: boolean }>(clearCommand("NotepadState")),
    clearPCADatabase: () => execute<{ status: string; removedFiles: number }>(clearCommand("PCADatabase")),
    invokeCrashDumpErase: () => execute<{ status: string; removedItems: number }>(invokeCommand("CrashDumpErase")),
    invokeVirtualMemoryPurge: () => execute<{ status: string; results: Record<string, boolean> }>(invokeCommand("VirtualMemoryPurge")),
    getVirtualMemoryStatus: () => execute<{ hiberEnabled: boolean; hiberFileExists: boolean; swapFileExists: boolean; clearPageFileAtShutdown: boolean }>("Get-VirtualMemoryStatus"),
    clearSearchIndex: () => execute<{ status: string; removedItems: number }>(clearCommand("SearchIndex")),
    clearPrintSpooler: () => execute<{ status: string; removedItems: number }>(clearCommand("PrintSpooler")),
    invokeSQLiteWALKiller: () => execute<{ status: string; killedFiles: number; skippedLocked: number }>(invokeCommand("SQLiteWALKiller")),
    clearRecallDatabase: () => execute<{ status: string; removedFiles: number }>(clearCommand("RecallDatabase")),
    clearWebCache: () => execute<{ ok: boolean; stdout: string }>(clearCommand("WebCache")),
    clearThumbnailCache: () => execute<{ ok: boolean; stdout: string }>(clearCommand("ThumbnailCache")),
    clearNotificationDb: () => execute<{ ok: boolean; stdout: string }>(clearCommand("NotificationDatabase")),
    clearBranchCache: () => execute<{ ok: boolean; stdout: string }>(clearCommand("BranchCache")),
    clearEventTranscript: () => execute<{ ok: boolean; stdout: string }>(clearCommand("EventTranscript")),
    clearActivitiesTimeline: () => execute<{ ok: boolean; stdout: string }>(clearCommand("ActivitiesTimeline")),
    clearRdpBitmapCache: () => execute<{ ok: boolean; stdout: string }>(clearCommand("RdpBitmapCache")),
    clearServicingLogs: () => execute<{ ok: boolean; stdout: string }>(clearCommand("ServicingLogs")),
    clearDeviceInstallLogs: () => execute<{ ok: boolean; stdout: string }>(clearCommand("DeviceInstallLogs")),
    clearUsageTraceLogs: () => execute<{ ok: boolean; stdout: string }>(clearCommand("UsageTraceLogs")),
    clearDefenderHistory: () => execute<{ ok: boolean; stdout: string }>(clearCommand("DefenderHistory")),
    clearWSLData: () => execute<{ ok: boolean; stdout: string }>(clearCommand("WSLData")),
    clearDockerDesktopData: () => execute<{ ok: boolean; stdout: string }>(clearCommand("DockerDesktopData")),
    clearVirtualMachineArtifacts: () => execute<{ ok: boolean; stdout: string }>(clearCommand("VirtualMachineArtifacts")),
    clearDeveloperCaches: () => execute<{ ok: boolean; stdout: string }>(clearCommand("DeveloperCaches")),
    clearCredentialManager: () => execute<{ ok: boolean; stdout: string }>(clearCommand("CredentialManager")),
    clearNetworkWizardHistory: () => execute<{ ok: boolean; stdout: string }>(clearCommand("NetworkWizardHistory")),
    clearWERHistory: () => execute<{ ok: boolean; stdout: string }>(clearCommand("WERHistory")),
    clearInactiveUserProtectionMetadata: () => execute<{ ok: boolean; stdout: string }>(clearCommand("InactiveUserProtectionMetadata")),
    clearStickyNotes: () => execute<{ ok: boolean; stdout: string }>(clearCommand("StickyNotes")),
    clearOneDriveMetadata: () => execute<{ ok: boolean; stdout: string }>(clearCommand("OneDriveMetadata")),
    clearSpotlightCache: () => execute<{ ok: boolean; stdout: string }>(clearCommand("SpotlightCache")),
    clearFontCache: () => execute<{ ok: boolean; stdout: string }>(clearCommand("FontCache")),
    clearLegacyIconCache: () => execute<{ ok: boolean; stdout: string }>(clearCommand("LegacyIconCache")),
    clearGameCaptures: () => execute<{ ok: boolean; stdout: string }>(clearCommand("GameCaptures")),
    clearPhotosCache: () => execute<{ ok: boolean; stdout: string }>(clearCommand("PhotosCache")),
    clearXboxCache: () => execute<{ ok: boolean; stdout: string }>(clearCommand("XboxCache")),
    clearCommunicationCaches: () => execute<{ ok: boolean; stdout: string }>(clearCommand("CommunicationCaches")),
    clearEditorHistory: () => execute<{ ok: boolean; stdout: string }>(clearCommand("EditorHistory")),
    clearGitActivity: () => execute<{ ok: boolean; stdout: string }>(clearCommand("GitActivity")),
    clearSSHState: () => execute<{ ok: boolean; stdout: string }>(clearCommand("SSHState")),
    clearRemoteAccessLogs: () => execute<{ ok: boolean; stdout: string }>(clearCommand("RemoteAccessLogs")),
    clearPasswordManagerCaches: () => execute<{ ok: boolean; stdout: string }>(clearCommand("PasswordManagerCaches")),
    clearGameLauncherLogs: () => execute<{ ok: boolean; stdout: string }>(clearCommand("GameLauncherLogs")),
    clearAdobeRecent: () => execute<{ ok: boolean; stdout: string }>(clearCommand("AdobeRecent")),
    clearOfficeTempFiles: () => execute<{ ok: boolean; stdout: string }>(clearCommand("OfficeTempFiles")),
    clearFirewallLog: () => execute<{ ok: boolean; stdout: string }>(clearCommand("FirewallLog")),
    clearNeighborCache: () => execute<{ ok: boolean; stdout: string }>(clearCommand("NeighborCache")),
    clearNetBIOSCache: () => execute<{ ok: boolean; stdout: string }>(clearCommand("NetBIOSCache")),
    clearGeolocationCache: () => execute<{ ok: boolean; stdout: string }>(clearCommand("GeolocationCache")),
    clearVPNPhonebooks: () => execute<{ ok: boolean; stdout: string }>(clearCommand("VPNPhonebooks")),
    clearProxyCache: () => execute<{ ok: boolean; stdout: string }>(clearCommand("ProxyCache")),
    clearCloudPlaceholders: () => execute<{ ok: boolean; stdout: string }>(clearCommand("CloudPlaceholders")),
    clearBITSQueue: () => execute<{ ok: boolean; stdout: string }>(clearCommand("BITSQueue")),
    clearCellularHistory: () => execute<{ ok: boolean; stdout: string }>(clearCommand("CellularHistory")),
    clearAppLaunchHistory: () => execute<{ ok: boolean; stdout: string }>(clearCommand("AppLaunchHistory")),
    clearOfficeMru: () => execute<{ ok: boolean; stdout: string }>(clearCommand("OfficeMru")),
    clearEmbeddedWebCache: () => execute<{ ok: boolean; stdout: string }>(clearCommand("EmbeddedWebCache")),
    clearP2PUpdateCache: () => execute<{ ok: boolean; stdout: string }>(clearCommand("P2PUpdateCache")),
    clearReliabilityHistory: () => execute<{ ok: boolean; stdout: string }>(clearCommand("ReliabilityHistory")),
    clearExplorerSearchHistory: () => execute<{ ok: boolean; stdout: string }>(clearCommand("ExplorerSearchHistory")),
    clearSearchPersonalizationData: () => execute<{ ok: boolean; stdout: string }>(clearCommand("SearchPersonalizationData")),
    invokeUnallocatedSpaceErase: (driveLetter?: string, mediaType?: string) =>
      execute<{ status: string; pid: number; drive: string; message: string }>(
        invokeCommand("UnallocatedSpaceErase"),
        {
          ...(driveLetter ? { DriveLetter: driveLetter } : {}),
          ...(mediaType   ? { MediaType: mediaType }     : {}),
        }
      ),
    invokeSSDTrim: () => execute<{ status: string; drives: Array<{ drive: string; status: string; reason?: string }> }>(invokeCommand("SSDTrim")),
    invokePreviousWindowsInstallErase: () => execute<{ status: string; freedMB: number }>(invokeCommand("PreviousWindowsInstallErase")),
    getPreviousWindowsInstallInfo: () => execute<{ present: boolean; sizeMB: number }>("Get-PreviousWindowsInstallInfo"),

    // Encryption Engine
    getEncryptionStatus: () => execute<EncryptionStatus>("Get-EncryptionStatus"),
    testEncryptionInstalled: () => execute<EncryptionStatus>("Test-EncryptionInstalled"),
    listEncryptionVolumes: () => execute<{ volumes: NonNullable<EncryptionStatus['volumes']> }>("List-EncryptionVolumes"),
    mountVolume: (path: string, letter: string, password: string, keyfile?: string, pim?: string) =>
      execute("Mount-EncryptionVolume", {
        VolumePath: path,
        DriveLetter: letter,
        Password: password,
        ...(keyfile ? { Keyfile: keyfile } : {}),
        ...(pim ? { Pim: pim } : {}),
      }),
    dismountVolume: (letter: string) =>
      execute("Dismount-EncryptionVolume", { DriveLetter: letter }),
    openEncryptionVolume: (letter: string) =>
      execute("Open-EncryptionVolume", { DriveLetter: letter }),
    dismountAllVolumes: () => execute(commandId("Dismount-", "All", "Encryption", "Volumes")),
    createVolume: (params: CreateVolumeParams) =>
      execute("Create-EncryptionVolume", {
        Path: params.Path,
        SizeMB: parseSizeToMB(params.Size),
        Password: params.Password,
        Encryption: params.Encryption,
        Hash: params.Hash,
        Filesystem: params.Filesystem,
        Quick: params.Quick,
        ...(params.Keyfile ? { Keyfile: params.Keyfile } : {}),
        ...(params.Pim ? { Pim: params.Pim } : {}),
      }),
    createDualVolume: (params: CreateDualVolumeParams) =>
      execute("Create-DualVolume", {
        Path: params.Path,
        FirstPassword: params.FirstPassword,
        SecondPassword: params.SecondPassword,
        HostSizeMB: parseSizeToMB(params.HostSize),
        SecondSizeMB: parseSizeToMB(params.SecondSize),
        Encryption: params.Encryption,
        Hash: params.Hash,
        Filesystem: params.Filesystem,
      }),
    createStegoMp4: (params: CreateStegoMp4Params) =>
      execute("Create-StegoMp4", {
        CarrierMp4: params.carrierMp4,
        OutputPath: params.outputPath,
        SizeMB: parseSizeToMB(params.size),
        Password: params.password,
      }),
    extractStegoMp4: (params: ExtractStegoMp4Params) =>
      execute("Extract-StegoMp4", {
        InputPath: params.inputPath,
        OutputPath: params.outputPath,
      }),
    getVolumeInfo: (letter: string) =>
      execute<VolumeInfo>("Get-VolumeInfo", { DriveLetter: letter }),
    getSystemEncryptionStatus: () =>
      execute<SystemEncryptionStatus>("Get-SystemEncryptionStatus"),
    getEncryptionPartitions: () =>
      execute<{ partitions: EncryptionPartition[] }>("Get-EncryptionPartitions"),
    getAvailableDriveLetters: () =>
      execute<{ letters: string[] }>("Get-AvailableDriveLetters"),
    getBitLockerVolumes: () =>
      execute<BitLockerVolume[]>("Get-BitLockerVolumes"),
    eraseEncryptedContainer: async (input: EraseInput): Promise<BackendResponse<EraseReceipt>> => {
      try {
        const data = await invoke<EraseReceipt>("erase_encrypted_container", { target: input });
        return { success: true, data };
      } catch (e) {
        return { success: false, error: e instanceof Error ? e.message : String(e) };
      }
    },

    // RAM Disks (ImDisk)
    testRamDiskInstalled: () => execute<{ installed?: boolean } | boolean>("Test-RamDiskInstalled"),
    installRamDiskEngine: () => execute<{ status: string; package?: string; error?: string }>("Install-RamDiskEngine"),
    getRamDiskStatus: () => execute<RamDiskStatus>("Get-RamDiskStatus"),
    getSystemRamInfo: () => execute<SystemRamInfo>("Get-SystemRamInfo"),
    createRamDisk: (params: CreateRamDiskParams) =>
      execute("New-RamDisk", {
        SizeMB: params.SizeMB,
        DriveLetter: params.DriveLetter,
        Filesystem: params.Filesystem,
        Label: params.Label,
        ReadOnly: params.ReadOnly,
        Quick: params.Quick,
      }),
    removeRamDisk: (letter: string) =>
      execute("Remove-RamDisk", { DriveLetter: letter }),
    removeAllRamDisks: () => execute("Remove-AllRamDisks"),
    openRamDisk: (letter: string) =>
      execute("Open-RamDisk", { DriveLetter: letter }),

    // MeshVPN (Private Mesh)
    getMeshVPNStatus: () => execute<MeshVPNStatus>("Get-MeshVPNStatus"),
    setMeshVPNConfig: (config: {
      AdvertiseExitNode: boolean;
      AllowLanAccess: boolean;
      Unattended: boolean;
      AcceptRoutes: boolean;
      ExitNodeIP: string;
      ShieldsUp: boolean;
      AcceptDNS: boolean;
      Force?: boolean;
    }) =>
      execute("Set-MeshVPNConfig", {
        AdvertiseExitNode: config.AdvertiseExitNode,
        AllowLanAccess: config.AllowLanAccess,
        Unattended: config.Unattended,
        AcceptRoutes: config.AcceptRoutes,
        ExitNodeIP: config.ExitNodeIP || "",
        ShieldsUp: config.ShieldsUp,
        AcceptDNS: config.AcceptDNS,
        Force: config.Force !== false,
      }),
    sendMeshVPNFile: (path: string, target: string) =>
      execute("Send-MeshVPNFile", { Path: path, Target: target }),
    startMeshVPNLogin: () => execute<{ url?: string; qrUrl?: string; output?: string }>("Start-MeshVPNLogin"),
    startMeshService: () => execute<{ success: boolean; error?: string }>("Start-MeshService"),
    stopMeshService: () => execute<{ success: boolean; error?: string }>("Stop-MeshService"),
    connectMeshVPN: () => execute<{ success: boolean; launched?: boolean; message?: string; pid?: number; error?: string; errorCode?: string }>("Connect-MeshVPN"),

    // Firewall
    getFirewallStatus: () => execute<FirewallStatus>("Get-FirewallStatus"),
    getFirewallRules: () => execute<{ rules: any[] }>("Get-FirewallRules"),
    addFirewallBlockRule: (name: string, path: string) =>
      execute("Add-FirewallBlockRule", { Name: name, Path: path }),
    setFirewallRuleEnabled: (name: string, enabled: boolean) =>
      execute("Set-FirewallRuleEnabled", { Name: name, Enabled: enabled ? "true" : "false" }),
    removeFirewallRule: (name: string) =>
      execute("Remove-FirewallRule", { Name: name }),
    enableLockdownMode: () => execute("Enable-LockdownMode"),
    disableLockdownMode: () => execute("Disable-LockdownMode"),

    // Firewall Protocol Blocking
    blockProtocol: (name: string, port: string | number, protocol: string, direction: "Both" | "Inbound" | "Outbound" = "Both") =>
      execute("Block-Protocol", { Name: name, Port: port, Protocol: protocol, Direction: direction }),
    unblockProtocol: (name: string) =>
      execute("Unblock-Protocol", { Name: name }),
    getProtocolBlocks: () => execute<{ blocks: { Name: string; Protocol: string; Port: string; Enabled: boolean; Direction: string }[] }>("Get-ProtocolBlocks"),


    // DNS
    getDNSStatus: () => execute<DNSStatus>("Get-DNSStatus"),
    setSecureDNS: (
      provider: string,
      options?: {
        dohId?: string;
        deviceName?: string;
        primary?: string;
        secondary?: string;
        primary6?: string;
        secondary6?: string;
        filterSlug?: string;
        silent?: boolean;
      }
    ) =>
      execute("Set-SecureDNS", {
        Provider: provider,
        ...(options?.dohId ? { DohId: options.dohId } : {}),
        ...(options?.deviceName ? { DeviceName: options.deviceName } : {}),
        ...(options?.primary ? { Primary: options.primary } : {}),
        ...(options?.secondary ? { Secondary: options.secondary } : {}),
        ...(options?.primary6 ? { Primary6: options.primary6 } : {}),
        ...(options?.secondary6 ? { Secondary6: options.secondary6 } : {}),
        ...(options?.filterSlug ? { FilterSlug: options.filterSlug } : {}),
        ...(options?.silent ? { Silent: true } : {}),
      }),
    clearSecureDNS: () => execute("Clear-SecureDNS"),
    enableIPv4Preference: () => execute("Enable-IPv4Preference"),
    disableIPv4Preference: () => execute("Disable-IPv4Preference"),
    enableDnsCensorshipProtection: () => execute("Enable-DNSCensorshipProtection"),
    disableDnsCensorshipProtection: () => execute("Disable-DNSCensorshipProtection"),

    // Network — live ports / connections viewer
    getNetworkPorts: (maxRows?: number) =>
      execute<NetworkPortsResult>("Get-NetworkPorts", maxRows ? { MaxRows: maxRows } : {}),

    // Dashboard — battery health
    getBatteryHealth: () => execute<BatteryHealthResult>("Get-BatteryHealth"),

    // Tweaks — Defender exclusion auditor (read-only)
    getDefenderExclusions: () => execute<DefenderExclusionsResult>("Get-DefenderExclusions"),

    // Network — adapter MAC randomizer
    getPhysicalNetworkAdapters: () =>
      execute<PhysicalNetworkAdaptersResult>("Get-PhysicalNetworkAdapters"),
    setAdapterRandomMAC: (adapterId: string, mode: MacRandomizerMode) =>
      execute<{ status: "ok" | "partial"; appliedMac?: string; mode?: MacRandomizerMode; warning?: string }>(
        "Set-AdapterRandomMAC",
        { AdapterId: adapterId, Mode: mode }
      ),
    restoreAdapterMAC: (adapterId: string) =>
      execute<{ status: "ok" | "partial"; warning?: string }>(
        "Restore-AdapterMAC",
        { AdapterId: adapterId }
      ),

    // App Installation
    getAppManifest: (category?: string) =>
      execute<AppManifest>("Get-AppManifest", category ? { Category: category } : {}),
    testWingetInstalled: () => execute<{ status: "installed" | "not-installed" }>("Test-WingetInstalled"),
    installWinget: () => execute<{ status: "installed" }>("Install-Winget"),
    installWingetApps: (appIds: string[]) =>
      execute("Install-WingetApps", { AppIds: appIds.join(",") }),
    // LEARNING: Get-AppInventory is the unified scan function that replaces the old
    // Get-AppStatus + Get-UpgradeList + Get-EssentialAppsStatus + Get-InstalledApps.
    // Returns the full AppInventorySnapshot which gets auto-persisted to
    // current.apps.inventory by the Rust backend after execution.
    // Takes 5-10s because of winget list + winget upgrade.
    // Call on startup + 60-min interval + after install/upgrade/uninstall.
    getAppInventory: () => execute<import('../types/settings').AppInventorySnapshot>("Get-AppInventory"),
    upgradeAllApps: () => execute("Upgrade-AllApps"),
    upgradeApp: (appId: string) => execute("Upgrade-App", { AppId: appId }),

    // Activation
    openActivationSettings: () => execute("Open-ActivationSettings"),


    // Blocklists
    getBlocklistStatus: () => execute<BlocklistStatus>("Get-BlocklistStatus"),
    // Version Check & Update
    getVersionUpdate: () => execute<{
      currentVersion: string;
      latestVersion: string;
      updateAvailable: boolean;
      downloadUrl: string;
      releaseName?: string;
      releaseNotes?: string;
    }>("Get-VersionUpdate"),
    openUpdatePage: (url?: string) => execute("Open-UpdatePage", url ? { Url: url } : {}),

    // Hosts File Blocklist Management
    addBlocklistToHosts: (blocklistName: string) => execute("Add-BlocklistToHosts", { BlocklistName: blocklistName }),
    removeBlocklistFromHosts: (blocklistName: string) => execute("Remove-BlocklistFromHosts", { BlocklistName: blocklistName }),

    // New Tweaks
    invokeSystemRepair: () => execute("Invoke-SystemRepair"),
    invokeWindowsUpdateRepair: () => execute("Invoke-WindowsUpdateRepair"),
    invokeDefrag: () => execute("Invoke-Defrag"),
    removeEdge: () => execute("Invoke-RemoveEdge"),
    removeOneDrive: () => execute("Invoke-RemoveOneDrive"),
    testEdgeInstalled: () => execute<{ installed: boolean }>("Test-EdgeInstalled"),
    testOneDriveInstalled: () => execute<{ installed: boolean }>("Test-OneDriveInstalled"),
    disableActivityHistory: () => execute("Disable-ActivityHistory"),
    enableActivityHistory: () => execute("Enable-ActivityHistory"),
    disableLocationTracking: () => execute("Disable-LocationTracking"),
    enableLocationTracking: () => execute("Enable-LocationTracking"),

    disableWindowsSuggestions: () => execute("Disable-WindowsSuggestions"),
    enableWindowsSuggestions: () => execute("Enable-WindowsSuggestions"),
    getWindowsSuggestionsStatus: () => execute<{ disabled: boolean }>("Get-WindowsSuggestionsStatus"),
    setAppCapabilityAccess: (capability: string, disabled: boolean) =>
      execute("Set-AppCapabilityAccess", { Capability: capability, Access: disabled ? "Deny" : "Allow" }),
    getAppCapabilityAccessStatus: (capability: string) =>
      execute<{ capability: string; value: string; disabled: boolean }>("Get-AppCapabilityAccessStatus", { Capability: capability }),
    getAppPrivacyCapabilitiesStatus: () =>
      execute<AppPrivacyCapabilitiesStatus>("Get-AppPrivacyCapabilitiesStatus"),

    disableAdvertisingID: () => execute("Disable-AdvertisingID"),
    enableAdvertisingID: () => execute("Enable-AdvertisingID"),
    disableTailoredExperiences: () => execute("Disable-TailoredExperiences"),
    enableTailoredExperiences: () => execute("Enable-TailoredExperiences"),
    disableOfficeLogging: () => execute("Disable-OfficeLogging"),
    enableOfficeLogging: () => execute("Enable-OfficeLogging"),
    disableDiagnosticEventTracing: () => execute("Disable-DiagnosticEventTracing"),
    enableDiagnosticEventTracing: () => execute("Enable-DiagnosticEventTracing"),
    createRestorePoint: () => execute("Create-RestorePoint"),
    disableLockScreenPrivacy: () => execute("Disable-LockScreenPrivacy"),
    enableLockScreenPrivacy: () => execute("Enable-LockScreenPrivacy"),
    getLockScreenPrivacyStatus: () => execute<{ disabled: boolean }>("Get-LockScreenPrivacyStatus"),
    disableSetupCompletionNags: () => execute("Disable-SetupCompletionNags"),
    enableSetupCompletionNags: () => execute("Enable-SetupCompletionNags"),
    getSetupCompletionNagsStatus: () => execute<{ disabled: boolean }>("Get-SetupCompletionNagsStatus"),
    enableEndTaskOnTaskbar: () => execute("Enable-EndTaskOnTaskbar"),
    disableEndTaskOnTaskbar: () => execute("Disable-EndTaskOnTaskbar"),
    disableBingSearch: () => execute("Disable-BingSearch"),
    enableBingSearch: () => execute("Enable-BingSearch"),
    showFileExtensions: () => execute("Show-FileExtensions"),
    hideFileExtensions: () => execute("Hide-FileExtensions"),
    showHiddenFiles: () => execute("Show-HiddenFiles"),
    hideHiddenFiles: () => execute("Hide-HiddenFiles"),
    enableDetailedBSOD: () => execute("Enable-DetailedBSOD"),
    disableDetailedBSOD: () => execute("Disable-DetailedBSOD"),
    enableUSBWriteProtect: () => execute("Enable-USBWriteProtect"),
    disableUSBWriteProtect: () => execute("Disable-USBWriteProtect"),
    enableUSBStorageLockdown: () => execute("Enable-USBStorageLockdown"),
    disableUSBStorageLockdown: () => execute("Disable-USBStorageLockdown"),
    setServicesManual: () => execute("Set-ServicesManual"),
    getProcessIntelligence: () => execute<{ processes: ProcessIntelligenceEntry[] }>("Get-ProcessIntelligence"),

    getPrivacyShieldStatus: () => execute<{
      running: boolean;
      cameraAvailable?: boolean;
      cameraDevices?: string[];
      cameraMessage?: string;
      config?: {
        modelLevel: string;
        confidence: number;
        overlayOpacity: number;
      };
      processId: number | null;
    }>("Get-PrivacyShieldStatus"),
    startPrivacyShield: (
      camera: number = 0,
      checkGaze: boolean = true,
      checkFaces: boolean = false,
      checkPhone: boolean = false,
      captureOnDevice: boolean = false,
      captureOnMultiFace: boolean = false,
      modelLevel: string = "medium",
      confidence: number = 0.5,
      overlayOpacity: number = 200,
      wakeDelayMs: number = 150,
      deviceWakeMultiplier: number = 2,
      multiFaceWakeMultiplier: number = 2,
      bufferFrames: number = 2,
      captureSpeed: number = 1
    ) => execute<{ success: boolean; processId?: number; message?: string }>("Start-PrivacyShield", {
      Camera: camera,
      CheckGaze: checkGaze,
      CheckFaces: checkFaces,
      CheckPhone: checkPhone,
      CaptureOnDevice: captureOnDevice,
      CaptureOnMultiFace: captureOnMultiFace,
      ModelLevel: modelLevel,
      Confidence: confidence,
      OverlayOpacity: overlayOpacity,
      WakeDelayMs: wakeDelayMs,
      DeviceWakeMultiplier: deviceWakeMultiplier,
      MultiFaceWakeMultiplier: multiFaceWakeMultiplier,
      BufferFrames: bufferFrames,
      CaptureSpeed: captureSpeed
    }),
    stopPrivacyShield: () => execute<{ success: boolean; message?: string }>("Stop-PrivacyShield"),
    installAIDependencies: (target?: string) => execute<{ success: boolean; message?: string; details?: string }>("Install-PrivacyShieldAI", target ? { Target: target } : {}),
    getAIDependenciesStatus: () => execute<{ installed: boolean; missing?: string[]; details?: { name: string; status: 'installed' | 'missing'; path?: string }[] }>("Get-AIDependenciesStatus"),
    getAppBranding: () => execute<AppBranding>("Get-AppBranding"),
    setAppBranding: (companyName: string, productName: string) =>
      execute<AppBranding>("Set-AppBranding", { CompanyName: companyName, ProductName: productName }),
    setOEMInformation: (model: string, manufacturer: string, supportUrl: string, supportProvider: string, logo?: string) =>
      execute("Set-OEMInformation", {
        Model: model,
        Manufacturer: manufacturer,
        SupportURL: supportUrl,
        SupportProvider: supportProvider,
        Logo: logo || ""
      }),
    setWinCommanderVisibility: (hidden: boolean) =>
      execute<{ status: string; itemsChanged: number; warnings?: string[] }>("Set-WinCommanderVisibility", { Hidden: hidden }),
    setWinCommanderCalculatorShortcuts: (hidden: boolean) =>
      execute<{ status: string; itemsChanged: number; warnings?: string[] }>("Set-WinCommanderCalculatorShortcuts", { Hidden: hidden }),
    restartExplorer: () => execute("Restart-Explorer"),
    /** Opens a file/folder path in Explorer via the Rust `open_path` command
     *  (not a PowerShell dispatch — same underlying command every "Open in
     *  Explorer" / "Open folder" button in the app should route through). */
    openPath: (path: string) => execute("open_path", { path }),

    // Startup Manager
    getStartupItems: () => execute<{ items: StartupItem[] }>("Get-StartupItems"),
    disableStartupItem: (name: string, location: string) =>
      execute("Disable-StartupItem", { Name: name, Location: location }),
    enableStartupItem: (name: string) =>
      execute("Enable-StartupItem", { Name: name }),
    optimizeStartup: (aggressive: boolean) =>
      execute<{ DisabledCount: number; RamSavedMB: number }>("Invoke-OptimizeStartup", { Aggressive: aggressive }),

    // Context Menu & Shredder
    toggleContextMenu: (enable: boolean) => invoke("toggle_context_menu", { enable }),
    getContextMenuStatus: () => invoke<boolean>("get_context_menu_status"),
    // Scrub metadata right-click integration — parallel to the shredder
    // entry; writes its own HKCU\...\WinCommanderScrub keys.
    toggleScrubContextMenu: (enable: boolean) => invoke("toggle_scrub_context_menu", { enable }),
    getScrubContextMenuStatus: () => invoke<boolean>("get_scrub_context_menu_status"),
    // Safe Copy / Safe Paste right-click integration — one paired verb set.
    // Safe Copy records a selection; Safe Paste copies it into a folder keeping
    // exact names then scrubs the copies (scrub is Pro). See safe_clip.rs.
    toggleSafeCopyContextMenu: (enable: boolean) => invoke("toggle_safe_copy_context_menu", { enable }),
    getSafeCopyContextMenuStatus: () => invoke<boolean>("get_safe_copy_context_menu_status"),
    safeCopyRecord: (paths: string[]) => invoke<number>("safe_copy_record", { paths }),
    safeClipStatus: () => invoke<SafeClipStatus>("safe_clip_status"),
    safePastePrepare: (destDir: string) => invoke<SafePasteResult>("safe_paste_prepare", { destDir }),
    scrubMetadataPaths: (paths: string[], options: ScrubMetadataOptions) =>
      invoke<ScrubReportSummary>("scrub_metadata_paths", { paths, options }),
    // The secure shredder command accepts Type in {File, Registry,
    // RegistryProperty, Folder}; the "File" branch handles directories
    // internally via PSIsContainer recursion, so we always pass "File"
    // regardless of whether the target is a file or a folder.
    invoke7Erase: (path: string, _type: 'File' | 'Directory' = 'File') => execute(invokeCommand("7Erase"), { Path: path, Type: 'File' }),

    // App Licensing
    getLicenseStatus: () => invoke<AppLicenseStatus>("get_license_status"),
    activateAppLicense: (licenseKey: string) =>
      invoke<AppLicenseStatus>("activate_license", { licenseKey }),
    refreshAppLicense: () => invoke<AppLicenseStatus>("refresh_license"),
    clearAppLicenseCache: () => invoke<void>("clear_license_cache"),
    deactivateAppLicense: () => invoke<void>("deactivate_license"),
    startTrial: () => invoke<AppLicenseStatus>("start_trial"),
    connectRdp: (hostname: string) =>
      invoke<void>("connect_rdp", { hostname }),
    setRdpCredentials: (hostname: string, username: string, password: string) =>
      invoke<void>("set_rdp_credentials", { hostname, username, password }),
    // Routine cleaner — scan returns only display metadata and opaque item IDs.
    // The backend revalidates IDs against its short-lived process-local cache.
    routineCleanerScan: (categories?: RoutineCleanerCategory[]) =>
      invoke<RoutineCleanerScan>("routine_cleaner_scan", { categories }),
    routineCleanerClean: (itemIds: string[]) =>
      invoke<RoutineCleanerCleanResult>("routine_cleaner_clean", { itemIds }),
    routineCleanerCancel: () => invoke<void>("routine_cleaner_cancel"),
    packageUpdatesInventory: () => invoke<PackageUpdateInventory>("package_updates_inventory"),
    packageUpdatesApply: (updateIds: string[]) => invoke<PackageUpdateResult>("package_updates_apply", { updateIds }),
    packageUpdatesCancel: () => invoke<void>("package_updates_cancel"),
    firewallAuditPreview: () => invoke<FirewallAudit>("firewall_audit_preview"),
    firewallAuditRemediate: (ruleIds: string[], action: "enable" | "disable" | "remove") =>
      invoke<FirewallRemediation>("firewall_audit_remediate", { ruleIds, action }),
    firewallAuditCancel: () => invoke<void>("firewall_audit_cancel"),
    duplicateFinderScan: (roots: string[]) =>
      invoke<DuplicateScan>("duplicate_finder_scan", { roots }),
    duplicateFinderRemove: (fileIds: string[]) =>
      invoke<DuplicateRemoveResult>("duplicate_finder_remove", { fileIds }),
    duplicateFinderCancel: () => invoke<void>("duplicate_finder_cancel"),
    emptyFolderCleanerScan: (roots: string[]) =>
      invoke<EmptyFolderScan>("empty_folder_cleaner_scan", { roots }),
    emptyFolderCleanerRemove: (folderIds: string[]) =>
      invoke<EmptyFolderRemoveResult>("empty_folder_cleaner_remove", { folderIds }),
    emptyFolderCleanerCancel: () => invoke<void>("empty_folder_cleaner_cancel"),
    registryCleanerScan: () => invoke<RegistryCleanerScan>("registry_cleaner_scan"),
    registryCleanerRemove: (entryIds: string[]) =>
      invoke<RegistryCleanerResult>("registry_cleaner_remove", { entryIds }),
    explorerContextMenuScan: () =>
      invoke<ExplorerContextScan>("explorer_context_menu_scan"),
    explorerContextMenuRemediate: (
      action: "disable" | "enable" | "remove",
      entryIds: string[],
    ) => invoke<ExplorerContextResult>("explorer_context_menu_remediate", { action, entryIds }),
    malwareScanStart: (scope: "quick" | "full") =>
      invoke<MalwareScanResult>("malware_scan_start", { scope }),
    malwareScanStatus: (scanId: string) =>
      invoke<MalwareScanResult>("malware_scan_status", { scanId }),
    malwareAllowlistAdd: (sha256: string) =>
      invoke<{ sha256: string; allowlisted: boolean }>("malware_allowlist_add", { sha256 }),
    malwareAllowlistRemove: (sha256: string) =>
      invoke<{ sha256: string; removed: boolean }>("malware_allowlist_remove", { sha256 }),
    malwareQuarantine: (findingId: string) =>
      invoke<{ quarantineId: string; state: string }>("malware_quarantine", { findingId }),
    malwareQuarantineRestore: (quarantineId: string) =>
      invoke<{ quarantineId: string; state: string }>("malware_quarantine_restore", { quarantineId }),
    malwareQuarantineDelete: (quarantineId: string) =>
      invoke<{ quarantineId: string; state: string }>("malware_quarantine_delete", { quarantineId }),
    malwareQuarantineList: () =>
      invoke<{ entries: MalwareQuarantineEntry[] }>("malware_quarantine_list"),
    securityThreatSnapshot: () =>
      invoke<SecurityThreatSnapshot>("security_threat_snapshot"),
    securityCveSnapshot: () =>
      invoke<SecurityCveSnapshot>("security_cve_snapshot"),
    shortcutCleanerScan: () => invoke<ShortcutScan>("shortcut_cleaner_scan"),
    shortcutCleanerRemove: (ids: string[]) => invoke<ShortcutRemoveResult>("shortcut_cleaner_remove", { ids }),
    shortcutCleanerCancel: () => invoke<void>("shortcut_cleaner_cancel"),
    environmentCleanerScan: () => invoke<EnvironmentScan>("environment_cleaner_scan"),
    environmentCleanerRepair: (ids: string[]) => invoke<EnvironmentRepairResult>("environment_cleaner_repair", { ids }),
    uninstallLeftoversScan: () => invoke<UninstallLeftoverScan>("uninstall_leftovers_scan"),
    uninstallLeftoversRemove: (ids: string[]) => invoke<UninstallLeftoverRemoveResult>("uninstall_leftovers_remove", { ids }),
    uninstallLeftoversCancel: () => invoke<void>("uninstall_leftovers_cancel"),
    arpCacheScan: () => invoke<ArpScan>("arp_cache_scan"),
    arpCacheClear: (scanId: string) => invoke<ArpClearResult>("arp_cache_clear", { scanId }),
    startupImpactScan: () => invoke<StartupImpactScan>("startup_impact_scan"),
    driverMaintenanceInventory: () => invoke<DriverMaintenanceInventory>("driver_maintenance_inventory"),
    driverUpdateSeam: () => invoke<DriverUpdateSeam>("driver_update_seam"),
    /** Run System Cleaner to erase all browser caches/history and system traces (prefetch, logs, shellbags, etc.) */
    bleachbitClean: (excludeBrowsers: boolean, preview: boolean) =>
      invoke<{ summary: string; filesCount: number; spaceRecovered: string; specialOps: number; errors: number; preview: boolean; samplePaths: string[] }>("run_bleachbit_clean", { excludeBrowsers, preview }),

    // ── Disk Space Analyzer ────────────────────────────────────────────
    /** Run WizTree scan for a path. Returns metadata; children fetched via getDiskChildren. */
    runDiskScan: (path: string) =>
      invoke<{ scanRoot: string; totalSize: number; freeSpace: number; driveCapacity: number; fileCount: number; folderCount: number; wiztreeFound: boolean }>("run_disk_scan", { path }),
    /** Fetch sorted children of a folder from the in-memory scan cache. */
    getDiskChildren: (path: string) =>
      invoke<Array<{ name: string; fullPath: string; size: number; allocated: number; isDir: boolean; lastModified: string; fileCount: number; folderCount: number }>>("get_disk_children", { path }),
    /** Fetch a flat list of the largest files/folders from the latest WizTree scan. */
    getLargeDiskItems: (minSizeBytes: number, limit: number = 200, includeDirs: boolean = true) =>
      invoke<Array<{ name: string; fullPath: string; size: number; allocated: number; isDir: boolean; lastModified: string; fileCount: number; folderCount: number; itemType: string; cleanupHint: string; risk: string }>>("get_large_disk_items", { minSizeBytes, limit, includeDirs }),
    /** Permanently delete a file or folder and evict it from the scan cache. */
    diskDeleteItem: (path: string) =>
      invoke<void>("disk_delete_item", { path }),

    // Productivity
    getProductivityStatus: () => execute<{ running: boolean; details: { server: boolean; input: boolean; active: boolean } }>("Get-ProductivityStatus"),
    startProductivityTracker: () => execute("Start-ProductivityTracker"),
    stopProductivityTracker: () => execute("Stop-ProductivityTracker"),
    invokeProductivityEngineMaintenance: () => execute("Invoke-ProductivityEngineMaintenance"),

    // Settings Migration
    getMigrationData: () => execute<Record<string, any>>("Get-WCMigrationData"),

    // ══════════════════════════════════════════════════════════════════
    // NEW: ReviOS-ported features
    // ══════════════════════════════════════════════════════════════════

    // Privacy: Recall & Typing Insights
    disableRecallSnapshots: () => execute("Disable-RecallSnapshots"),
    enableRecallSnapshots: () => execute("Enable-RecallSnapshots"),
    disableTypingInsights: () => execute("Disable-TypingInsights"),
    enableTypingInsights: () => execute("Enable-TypingInsights"),
    getTypingInsightsStatus: () => execute<{ disabled: boolean }>("Get-TypingInsightsStatus"),

    // Privacy: Internet Communication Restrictions
    disableInternetCommunication: () => execute("Disable-InternetCommunication"),
    enableInternetCommunication: () => execute("Enable-InternetCommunication"),

    // UI: Explorer Enhancements
    disableFolderTypeDiscovery: () => execute("Disable-FolderTypeDiscovery"),
    enableFolderTypeDiscovery: () => execute("Enable-FolderTypeDiscovery"),
    removeShortcutSuffix: () => execute("Remove-ShortcutSuffix"),
    restoreShortcutSuffix: () => execute("Restore-ShortcutSuffix"),
    disableAutoPlay: () => execute("Disable-AutoPlay"),
    enableAutoPlay: () => execute("Enable-AutoPlay"),
    disableLowDiskCheck: () => execute("Disable-LowDiskCheck"),
    enableLowDiskCheck: () => execute("Enable-LowDiskCheck"),
    setExplorerOpensThisPC: () => execute("Set-ExplorerOpensThisPC"),
    setExplorerOpensQuickAccess: () => execute("Set-ExplorerOpensQuickAccess"),
    hideSyncProviderNotifications: () => execute("Hide-SyncProviderNotifications"),
    showSyncProviderNotifications: () => execute("Show-SyncProviderNotifications"),
    disableTransparencyEffects: () => execute("Disable-TransparencyEffects"),
    enableTransparencyEffects: () => execute("Enable-TransparencyEffects"),
    enableFullPathInTitleBar: () => execute("Enable-FullPathInTitleBar"),
    disableFullPathInTitleBar: () => execute("Disable-FullPathInTitleBar"),
    setTaskbarDebloated: () => execute("Set-TaskbarDebloated"),
    resetTaskbarDebloated: () => execute("Reset-TaskbarDebloated"),
    disableStartRecommendations: () => execute("Disable-StartRecommendations"),
    enableStartRecommendations: () => execute("Enable-StartRecommendations"),

    // Security: New Hardening
    disableVBS: () => execute("Disable-VBS"),
    enableVBS: () => execute("Enable-VBS"),
    disableBitLockerAutoEncrypt: () => execute("Disable-BitLockerAutoEncrypt"),
    enableBitLockerAutoEncrypt: () => execute("Enable-BitLockerAutoEncrypt"),
    scanAcquisitionThreats: () => execute("Scan-AcquisitionThreats", {}),
    enableFullDiskEncryption: (pin: string) => execute("Enable-FullDiskEncryption", { Pin: pin, Drive: "C:" }),
    disableWPBT: () => execute("Disable-WPBT"),
    enableWPBT: () => execute("Enable-WPBT"),
    disableSmartScreen: () => execute("Disable-SmartScreen"),
    enableSmartScreen: () => execute("Enable-SmartScreen"),
    setOOBEBypass: () => execute("Set-OOBEBypass"),
    clearOOBEBypass: () => execute("Clear-OOBEBypass"),
    disableGameDVR: () => execute("Disable-GameDVR"),
    enableGameDVR: () => execute("Enable-GameDVR"),

    // Browser Hardening — dynamic (detect installed browsers, harden by name)
    getInstalledBrowsers: () => execute<{ browsers: InstalledBrowser[]; count: number }>("Get-InstalledBrowsersJson"),
    hardenBrowserByName: (name: string) => execute("Enable-HardenBrowserByName", { Name: name }),
    restoreBrowserByName: (name: string) => execute("Disable-HardenBrowserByName", { Name: name }),
    installUniversalBrowserExtensions: () => execute("Install-UniversalBrowserExtensions"),
    removeUniversalBrowserExtensions: () => execute("Remove-UniversalBrowserExtensions"),

    // Copilot / AI Component Removal
    removeCopilotAIComponents: () => execute("Remove-CopilotAIComponents"),
    restoreCopilotAIComponents: () => execute("Restore-CopilotAIComponents"),

    // OS: Performance
    disableMemoryCompression: () => execute("Disable-MemoryCompression"),
    enableMemoryCompression: () => execute("Enable-MemoryCompression"),
    setWin32PrioritySeparation: () => execute("Set-Win32PrioritySeparation"),
    resetWin32PrioritySeparation: () => execute("Reset-Win32PrioritySeparation"),
    setOptimizedTimeouts: () => execute("Set-OptimizedTimeouts"),
    resetOptimizedTimeouts: () => execute("Reset-OptimizedTimeouts"),
    disableReservedStorage: () => execute("Disable-ReservedStorage"),
    enableReservedStorage: () => execute("Enable-ReservedStorage"),

    // Boot & Kernel
    enableTSX: () => execute("Enable-TSX"),
    disableTSX: () => execute("Disable-TSX"),
    disableFirstLogonAnimation: () => execute("Disable-FirstLogonAnimation"),
    enableFirstLogonAnimation: () => execute("Enable-FirstLogonAnimation"),
    disableStartupSound: () => execute("Disable-StartupSound"),
    enableStartupSound: () => execute("Enable-StartupSound"),
    disableAutoRestartSignon: () => execute("Disable-AutoRestartSignon"),
    enableAutoRestartSignon: () => execute("Enable-AutoRestartSignon"),
    disableAutoRebootOnBSOD: () => execute("Disable-AutoRebootOnBSOD"),
    enableAutoRebootOnBSOD: () => execute("Enable-AutoRebootOnBSOD"),
    setSmallMemoryDump: () => execute("Set-SmallMemoryDump"),
    resetSmallMemoryDump: () => execute("Reset-SmallMemoryDump"),

    // Apps: Teams & APPX Debloat
    removeTeams: () => execute("Remove-MicrosoftTeams"),
    getTeamsStatus: () => execute<{ installed: boolean }>("Get-TeamsStatus"),
    getInstalledAppxInventory: () => execute<{ apps: { name: string; packageFullName: string; isProvisioned: boolean }[] }>("Get-InstalledAppxInventory"),
    removeAppxByName: (name: string) => execute("Remove-AppxByName", { Name: name }),
    restoreAppxByName: (name: string) => execute("Restore-AppxByName", { Name: name }),
    setAppxDeprovisioned: (name: string) => execute("Set-AppxDeprovisioned", { Name: name }),

    // Apps: BCU (Bulk Crap Uninstaller)
    testBcuInstalled: () => execute<{ installed: boolean; cliPath: string }>("Test-BcuInstalled"),
    installBcu: () => execute<{ status: string; cliPath?: string }>("Install-BcuUninstaller"),
    getBcuApplicationList: () => execute<BcuApplicationListResult>("Get-BcuApplicationList"),
    bcuUninstall: (names: string, quiet?: boolean, removeLeftovers?: boolean, leftoverLevel?: string) =>
      execute("Invoke-BcuUninstall", {
        Names: names,
        Quiet: quiet !== false ? "true" : "false",
        RemoveLeftovers: removeLeftovers !== false ? "true" : "false",
        LeftoverLevel: leftoverLevel || "VeryGood",
      }),
    bcuQuietUninstallSingle: (name: string, removeLeftovers?: boolean, leftoverLevel?: string) =>
      execute("Invoke-BcuQuietUninstallSingle", {
        Name: name,
        RemoveLeftovers: removeLeftovers !== false ? "true" : "false",
        LeftoverLevel: leftoverLevel || "VeryGood",
      }),
    bcuLoudUninstallSingle: (name: string, removeLeftovers?: boolean, leftoverLevel?: string) =>
      execute("Invoke-BcuLoudUninstallSingle", {
        Name: name,
        RemoveLeftovers: removeLeftovers !== false ? "true" : "false",
        LeftoverLevel: leftoverLevel || "VeryGood",
      }),
    bcuCleanupProgramFiles: () => execute<BcuProgramFilesCleanupResult>("Invoke-BcuCleanupProgramFiles"),
    bcuRemoveOrphanedFolder: (path: string) => execute("Remove-BcuOrphanedFolder", { Path: path }),
    getBcuWindowsFeatures: () => execute<BcuWindowsFeaturesResult>("Get-BcuWindowsFeatures"),
    disableBcuWindowsFeature: (featureName: string) => execute("Disable-BcuWindowsFeature", { FeatureName: featureName }),
    enableBcuWindowsFeature: (featureName: string) => execute("Enable-BcuWindowsFeature", { FeatureName: featureName }),
    getBcuStartupItems: () => execute<BcuStartupItemsResult>("Get-BcuStartupItems"),
    removeBcuStartupItem: (name: string, location: string, type: string) =>
      execute("Remove-BcuStartupItem", { Name: name, Location: location, Type: type }),
    getBcuRegistryLeftovers: () => execute<BcuRegistryLeftoversResult>("Get-BcuRegistryLeftovers"),
    removeBcuRegistryLeftover: (registryPath: string) => execute("Remove-BcuRegistryLeftover", { RegistryPath: registryPath }),
    exportBcuApplicationList: (outputPath?: string) => execute("Export-BcuApplicationList", { OutputPath: outputPath || "" }),

    // App exit
    exitApp: () => invoke("exit_app"),

    // Raw execute for custom commands
    execute,
  }), [execute]);

  return {
    ...backend,
    loading,
    error,
  };
}


// Execute backend command outside of React hook context

export async function executeBackendCommand<T>(
  command: string,
  params: Record<string, string | number | boolean> = {}
): Promise<BackendResponse<T>> {
  try {
    const result = await runBackendScriptTracked<any>(command, params);

    const isPoshError = result && typeof result === "object" && "error" in result && (result as { error?: boolean }).error;
    const isProOkFalse = result && typeof result === "object" && "ok" in result && (result as { ok?: boolean }).ok === false;
    if (isPoshError || isProOkFalse) {
      return { success: false, error: getErrorMessage(result) };
    }
    return { success: true, data: result as T };
  } catch (err) {
    return { success: false, error: await normalizeBackendError(err) };
  }
}

export default useBackend;
