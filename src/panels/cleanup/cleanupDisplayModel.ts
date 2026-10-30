import type { CleanupCategory } from "./cleanupCategories";

export type CleanupDisplayState = "scanning" | "not-scanned" | "unavailable" | "error" | "partial" | "limited" | "inconsistent" | "empty" | "found";
export type CleanupVerification = { status: "cleared" | "reduced" | "reset" | "unchanged" | "deferred" | "failed" | "unverified"; verifiedAt?: string };
type RecordValue = Record<string, unknown>;
const object = (value: unknown): value is RecordValue => !!value && typeof value === "object" && !Array.isArray(value);

export function hasLimitedCleanupCoverage(id: string, raw: unknown): boolean {
  return object(raw) && (raw.coverage === "limited" || (id === "srumData" && raw.historicalRecordsAvailable === false) || (id === "searchIndex" && typeof raw.coverage === "string" && raw.coverage.length > 0));
}

const SIMPLE_NAMES: Record<string, string> = {
  shellBags: "Folder history", usbHistory: "USB device history", dnsCache: "Web address cache",
  execCache: "App execution history", netDrives: "Saved network drives", rdpHistory: "Remote desktop history",
  jumpLists: "App recent and pinned items", prefetchFiles: "App startup history", shadowCopies: "Recovery snapshots",
  ntfsJournals: "Drive change journals", amcache: "App launch records", ntUserTraces: "Windows activity history",
  pcaDatabase: "App compatibility history", srumData: "Resource usage database", walFiles: "Temporary database files",
  webCache: "Browser data store", branchCache: "Shared download cache", eventTranscript: "Diagnostics history",
  rdpBitmapCache: "Remote desktop image cache", servicingLogs: "Windows update history logs", usageTraceLogs: "Device usage logs",
  wslData: "Linux data on Windows", dockerDesktopData: "Docker data", virtualMachineArtifacts: "Virtual machine data",
  inactiveUserProtectionMetadata: "Other users' encryption data", oneDriveMetadata: "OneDrive sync data",
  communicationCaches: "Chat and meeting app caches", gitActivity: "Git credentials and activity", sshState: "SSH connections and keys",
  neighborCache: "Nearby network devices", netbiosCache: "Local network name cache", geolocationCache: "Location cache",
  vpnPhonebooks: "Saved VPN connections", proxyCache: "Proxy connection settings", cloudPlaceholders: "Cloud sync placeholders",
  bitsQueue: "Background download queue", appLaunchHistory: "Programs launched", officeMru: "Office recent documents",
  embeddedWebCache: "In-app browser data", p2pUpdateCache: "Shared update cache", explorerSearchHistory: "File Explorer searches",
  searchPersonalization: "Search and typing history", unallocatedErase: "Erase free space", virtualMemory: "Memory files",
};

const SIMPLE_DESCRIPTIONS: Record<string, string> = {
  dnsCache: "Recently looked-up website and network addresses.", execCache: "Records Windows keeps when programs run.",
  prefetchFiles: "Files Windows uses to help apps start and records of recent starts.",
  shadowCopies: "Saved recovery snapshots that can help restore files.", ntfsJournals: "A record of changes made on a drive.",
  amcache: "Records Windows keeps about applications.", ntUserTraces: "Recent Run commands, typed paths and file picker history.",
  srumData: "The local resource usage database file; individual history records are not shown.",
  branchCache: "Copies of shared downloads stored on this device.", eventTranscript: "Windows diagnostics history.",
  wslData: "Linux distributions and their local storage.", dockerDesktopData: "Docker containers, images, storage and logs.",
  virtualMachineArtifacts: "Virtual machine snapshots, logs and settings.", inactiveUserProtectionMetadata: "Encryption data belonging to other Windows users.",
  gitActivity: "Saved Git credentials and repository activity.", sshState: "Saved SSH host information and agent state.",
  neighborCache: "Recently discovered devices and addresses on your network.", netbiosCache: "Names of devices recently found on your local network.",
  cloudPlaceholders: "Local placeholders used by cloud storage applications.", bitsQueue: "Windows background transfers and their queue files.",
  appLaunchHistory: "A Windows record of programs launched by users.", embeddedWebCache: "Browsing data stored inside other applications.",
  p2pUpdateCache: "Update files stored for sharing with other devices.",
};

export function cleanupDisplayDescription(category: Pick<CleanupCategory, "id" | "description">, mode: "simple" | "advanced"): string {
  if (category.id === "srumData") return SIMPLE_DESCRIPTIONS.srumData;
  return mode === "simple" ? (SIMPLE_DESCRIPTIONS[category.id] ?? category.description) : category.description;
}

const CLEAR_EXPLANATIONS: Record<string, string> = {
  wlanProfiles: "Forgets all-user Wi-Fi profiles on available adapters. Private and policy-managed profiles are excluded. You may need to reconnect and enter passwords again.",
  browserFootprints: "Removes browser data across supported browsers and profiles, including legacy Windows browser stores. The scan may not show every profile. This can sign you out and may remove saved passwords.",
  notepadState: "Removes temporary Notepad state, including unsaved tabs.",
  netDrives: "Removes saved network-drive connections. You may need to reconnect them.",
  eventLogs: "Clears Windows event records, including records used for troubleshooting or security review.",
  amcache: "Removes app launch records. Windows may create new records as programs run.",
  psHistory: "Removes saved terminal command history.",
  recentFiles: "Removes Windows recent-file shortcuts; it does not target the original files.",
  jumpLists: "Removes app recent and pinned-item lists.",
  printSpooler: "Removes temporary print queue files and may affect queued print jobs.",
  ntfsJournals: "Requests a reset of drive change journals. Apps that rely on them may need to rescan.",
  walFiles: "Removes temporary database files. Close affected apps first to reduce the risk of lost work.",
  webCache: "Removes cached web data and cookies, which can sign you out.",
  notificationDb: "Removes saved notification history.",
  rdpBitmapCache: "Removes cached images from remote desktop sessions.",
  officeMru: "Removes Office recent-document and trust records.",
  embeddedWebCache: "Removes browser data stored by apps and can sign you out.",
};

export function cleanupClearExplanation(category: Pick<CleanupCategory, "id" | "description" | "confirmMessage">, mode: "simple" | "advanced"): string {
  return CLEAR_EXPLANATIONS[category.id] ?? category.confirmMessage ?? `Requests removal of ${cleanupDisplayDescription(category, mode).replace(/[.]+$/, "").toLowerCase()}. Some items may return.`;
}

export function cleanupDisplayName(category: Pick<CleanupCategory, "id" | "label">, mode: "simple" | "advanced"): string {
  return mode === "simple" ? (SIMPLE_NAMES[category.id] ?? category.label) : category.label;
}

export function cleanupCountUnit(id: string, count: number): string {
  const units: Record<string, [string, string]> = {
    amcache: ["record", "records"], eventLogs: ["event record", "event records"],
    browserFootprints: ["browser", "browsers"], wlanProfiles: ["Wi-Fi profile", "Wi-Fi profiles"], clipboardHistory: ["clipboard item", "clipboard items"],
    psHistory: ["command line", "command lines"], srumData: ["database file", "database files"],
    appLaunchHistory: ["tracked program", "tracked programs"], ntfsJournals: ["active journal", "active journals"],
  };
  const [one, many] = units[id] ?? ["item", "items"];
  return count === 1 ? one : many;
}

/** The number of returned process rows, not a claim that enumeration was complete. */
export function observedProcessRows(id: string, raw: unknown): number | null {
  if (id !== "processIntel" || !object(raw) || raw.error || raw.status !== "partial" || raw.partial !== true || !Array.isArray(raw.processes)) return null;
  const seen = new Set<number>();
  for (const row of raw.processes) {
    if (!object(row) || !Number.isSafeInteger(row.pid) || (row.pid as number) <= 0 ||
        typeof row.name !== "string" || !row.name.trim() || seen.has(row.pid as number)) return null;
    seen.add(row.pid as number);
  }
  return seen.size;
}

export function cleanupVerificationText(receipt?: CleanupVerification): string {
  switch (receipt?.status) {
    case "cleared": return "A fresh scan found no remaining items.";
    case "reduced": return "A fresh scan found fewer items.";
    case "reset": return "The reset was verified.";
    case "unchanged": return "The fresh scan did not confirm a change.";
    case "deferred": return "The action is pending; verify after restart.";
    case "failed": return "The clear or verification failed.";
    case "unverified": return "The result could not be verified by a fresh scan.";
    default: return "No verification receipt reported.";
  }
}

export function deriveCleanupVerification(input: { id: string; before: number; result: CleanupDisplaySummary; backendSucceeded: boolean; deferred: boolean }): Exclude<CleanupVerification["status"], "reset"> {
  if (!input.backendSucceeded) return "failed";
  if (input.deferred) return "deferred";
  if (input.id === "ntfsJournals" || input.result.count === null || !["found", "empty"].includes(input.result.state)) return "unverified";
  if (input.result.count === 0) return "cleared";
  if (input.result.count < input.before) return "reduced";
  return "unchanged";
}

const AM_CACHE_NAMES: Record<string, string> = {
  InventoryApplication: "Applications", InventoryApplicationFile: "Application files",
  InventoryApplicationShortcut: "Shortcuts", InventoryDeviceContainer: "Devices",
  InventoryDriverBinary: "Drivers", Application: "Applications",
};
const USER_TRACE_NAMES: Record<string, string> = {
  RunMRU: "Run history", TypedPaths: "Typed folder paths", TypedURLs: "Typed web addresses", WordWheelQuery: "File Explorer searches",
};

export interface CleanupDisplaySummary {
  state: CleanupDisplayState;
  message: string;
  nextStep: string;
  count: number | null;
  sampleCount: number | null;
  categories: Array<{ label: string; count: number; samples: number | null }>;
  datasets: Array<{ label: string; rows: number }>;
  previewLimited: boolean;
  previewLimit: number | null;
}

export function summarizeCleanupGroups(count: number, groups: Array<{ count: number; items: string[]; rawData?: unknown }>): CleanupDisplaySummary {
  const groupRecords = groups.map(group => object(group.rawData) && Array.isArray(group.rawData.records) ? group.rawData.records : null);
  const sampleCount = groupRecords.length > 0 && groupRecords.every(records => records !== null)
    ? groupRecords.reduce<number>((sum, records) => sum + (records?.length ?? 0), 0) : null;
  const base = { count: null, sampleCount, categories: [], datasets: [], previewLimited: false, previewLimit: null };
  if (!Number.isFinite(count) || count < 0 || groups.some(group => !Number.isFinite(group.count) || group.count < 0)) {
    return { ...base, state: "partial", message: "Account scan incomplete", nextStep: "Review account coverage and scan again." };
  }
  if (!groups.length) return { ...base, state: "unavailable", message: "Account details unavailable", nextStep: "Scan again to verify the result." };
  if (groups.reduce((sum, group) => sum + group.count, 0) !== count) return { ...base, state: "inconsistent", message: "Counts do not agree", nextStep: "Scan again before clearing." };
  if (count === 0 && groups.some(group => object(group.rawData) && group.rawData.coverage === "limited")) return { ...base, count, state: "limited", message: "No items found in checked locations", nextStep: "This scan covers selected locations only." };
  if (count === 0) return { ...base, count, state: "empty", message: "No items found", nextStep: "No action is needed from this scan." };
  return { ...base, count, state: "found", message: `${count} ${count === 1 ? "item" : "items"} reported`, nextStep: "Review the account breakdown before clearing." };
}

/** A display projection only. It never changes the scan data or authorizes an operation. */
export function summarizeCleanupResult(input: { id: string; count: number; items?: string[]; raw?: unknown; error?: string; loading?: boolean }): CleanupDisplaySummary {
  const raw = object(input.raw) ? input.raw : undefined;
  const groupedKey = input.id === "amcache" ? "entries" : input.id === "ntUserTraces" ? "sections" : null;
  // Per-account summaries use `records` instead of the current-user scan's
  // `sections`. They may contain either grouped sections or flat records.
  const accountRecords = raw && Array.isArray(raw.records) ? raw.records : undefined;
  const accountActivity = input.id === "ntUserTraces" && raw && !Array.isArray(raw.sections) && accountRecords !== undefined;
  const groupedValue = groupedKey && raw ? (accountActivity && accountRecords?.some(row => object(row) && Array.isArray(row.entries)) ? accountRecords : raw[groupedKey]) : undefined;
  const rows = Array.isArray(groupedValue) ? groupedValue.filter(object) : [];
  const categories = rows.map(row => ({
    label: input.id === "amcache"
      ? (typeof row.category === "string" ? (AM_CACHE_NAMES[row.category] ?? "Other record category") : "Other record category")
      : (typeof row.name === "string" ? (USER_TRACE_NAMES[row.name] ?? "Other activity") : "Other activity"),
    count: typeof row.count === "number" && Number.isInteger(row.count) && row.count >= 0 ? row.count : -1,
    samples: Array.isArray(input.id === "amcache" ? row.sample : row.entries) ? (input.id === "amcache" ? row.sample as unknown[] : row.entries as unknown[]).length : null,
  }));
  const datasetNames: Record<string, string> = { entries: "Entries", files: "Files", records: "Records", logs: "Log channels", sections: "Sections", devices: "Devices", journals: "Journals", browsers: "Browsers", profiles: "Profiles", copies: "Recovery copies", dumps: "Crash reports", databases: "Databases", processes: "Processes", paths: "Paths" };
  const datasets = raw ? Object.entries(datasetNames).flatMap(([key, label]) => Array.isArray(raw[key]) ? [{ label, rows: (raw[key] as unknown[]).length }] : []) : [];
  const previewKeys = input.id === "wlanProfiles" ? ["profiles"] : input.id === "browserFootprints" ? ["browsers"] : ["records", "entries", "files", "logs", "devices", "journals", "copies", "dumps", "databases", "processes", "historyItems"];
  const previewArrays = raw ? previewKeys.map(key => raw[key]).filter(Array.isArray) : [];
  const previewRows = previewArrays.find(rows => rows.length > 0) ?? previewArrays[0];
  const sampleCount = groupedKey
    ? (categories.length && categories.every(row => row.samples !== null) ? categories.reduce((sum, row) => sum + (row.samples ?? 0), 0) : null)
    : previewRows?.length ?? null;
  const previewLimit = typeof raw?.previewLimit === "number" && Number.isFinite(raw.previewLimit) && raw.previewLimit >= 0 ? raw.previewLimit : null;
  const base = { count: Number.isFinite(input.count) && input.count >= 0 ? input.count : null, sampleCount, categories, datasets, previewLimited: raw?.truncated === true, previewLimit };
  if (input.loading) return { ...base, state: "scanning", count: null, message: "Scan in progress", nextStep: "Wait for the scan to finish." };
  if (raw?.error === true || (typeof raw?.error === "string" && raw.error.length > 0) || raw?.status === "failed" || raw?.status === "error") return { ...base, state: "error", count: null, message: "Scan failed", nextStep: "Try scanning again or check access." };
  if (raw?.status === "unavailable") return { ...base, state: "unavailable", count: null, message: "Scan unavailable", nextStep: "Try again when the source is available." };
  if (raw?.partial === true || raw?.complete === false || raw?.status === "partial" || raw?.coverage === "partial" || raw?.accessDenied === true) {
    const observed = observedProcessRows(input.id, raw);
    return { ...base, state: "partial", count: null, message: observed === null ? "Scan incomplete" : `${observed} ${observed === 1 ? "process" : "processes"} observed`, nextStep: observed === null ? "Review coverage and scan again." : "Some process details or processes could not be checked." };
  }
  if (input.error) return { ...base, state: "error", count: null, message: "Scan failed", nextStep: "Try scanning again or check access." };
  if (input.count < 0 || !Number.isFinite(input.count)) return { ...base, state: "not-scanned", count: null, message: "Not scanned", nextStep: "Select Scan to check this item." };
  if (!raw) return { ...base, state: "unavailable", count: null, message: "Scan details unavailable", nextStep: "Scan again to verify the result." };
  if (groupedKey && (!accountActivity || Array.isArray(groupedValue))) {
    // Only compare counts that the producer defines in the same units.
    // An absent or malformed breakdown is unknown, not evidence of zero.
    if (!Array.isArray(groupedValue) || rows.length !== groupedValue.length || (!accountActivity && (typeof raw.total !== "number" || !Number.isInteger(raw.total) || raw.total < 0)) || categories.some(row => row.count < 0) || categories.length === 0) {
      return { ...base, state: "unavailable", count: null, message: "Scan details unavailable", nextStep: "Scan again to verify the result." };
    }
    if ((!accountActivity && raw.total !== input.count) || categories.reduce((sum, row) => sum + row.count, 0) !== input.count) {
      return { ...base, state: "inconsistent", count: null, message: "Counts do not agree", nextStep: "Scan again before clearing." };
    }
  }
  if (input.count > 0 && !Object.values(raw).some(Array.isArray) && raw.previewLimit !== 0 && raw.sampleCount !== 0) return { ...base, state: "unavailable", count: null, message: "Scan details unavailable", nextStep: "Scan again to verify the result." };
  if (input.count > 0 && datasets.length && datasets.every(dataset => dataset.rows === 0) && raw.truncated !== true && raw.previewLimit !== 0 && raw.sampleCount !== 0) return { ...base, state: "unavailable", count: null, message: "Scan details unavailable", nextStep: "Scan again to verify the result." };
  if (input.count === 0 && !Object.values(raw).some(Array.isArray) && raw.fileExists !== false && raw.hveFileExists !== false) return { ...base, state: "unavailable", count: null, message: "Scan details unavailable", nextStep: "Scan again to verify the result." };
  if (input.count === 0 && hasLimitedCleanupCoverage(input.id, raw)) return { ...base, state: "limited", message: "No items found in checked locations", nextStep: "This scan covers selected locations only." };
  if (input.count === 0) return { ...base, state: "empty", message: "No items found", nextStep: "No action is needed from this scan." };
  return { ...base, state: "found", message: `${input.count} ${cleanupCountUnit(input.id, input.count)} found`, nextStep: "Review details before clearing." };
}
