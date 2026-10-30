import type { CleanupDisplayMode } from "../../panels/cleanup/cleanupDisplay";
import { useEffect, useMemo, useRef, useState } from "react";
import { useBackend } from "../../hooks/useBackend";
import { readCleanupUserSummary } from "../../panels/cleanup/cleanupScanResult";

type RecordObject = Record<string, unknown>;
type PreviewRow = { label: string; facts: string[] };
const isObject = (value: unknown): value is RecordObject => value !== null && typeof value === "object" && !Array.isArray(value);

function wifiProfiles(input: unknown): unknown[] {
  if (!isObject(input)) return [];
  const clearable = Array.isArray(input.profiles) ? input.profiles : Array.isArray(input.records) ? input.records : [];
  return [...clearable, ...(Array.isArray(input.additionalProfiles) ? input.additionalProfiles : [])];
}

// These are product-provided category labels, never values supplied by a scan.
const amcacheLabels: Record<string, string> = {
  InventoryApplication: "Application", InventoryApplicationFile: "Application file",
  InventoryApplicationShortcut: "Shortcut", InventoryDeviceContainer: "Device",
  InventoryDriverBinary: "Driver", Application: "Application",
};
const browserNames: Record<string, string> = {
  "google chrome": "Google Chrome", chrome: "Google Chrome", chromium: "Chromium",
  "microsoft edge": "Microsoft Edge", edge: "Microsoft Edge", brave: "Brave",
  "brave browser": "Brave", "mozilla firefox": "Mozilla Firefox", firefox: "Mozilla Firefox",
  opera: "Opera", "opera gx": "Opera GX", vivaldi: "Vivaldi",
};
const artifactNames: Record<string, string> = {
  history: "History", cookies: "Cookies", "login data": "Saved sign-ins",
  "top sites": "Top sites", favicons: "Site icons", "web data": "Web data",
  "visited links": "Visited links", shortcuts: "Shortcuts", cache: "Cache",
  "code cache": "Code cache", indexeddb: "Website storage", "local storage": "Local storage",
  "session storage": "Session storage", network: "Network data", "download metadata": "Download history",
  "places.sqlite": "History and bookmarks", "cookies.sqlite": "Cookies", "logins.json": "Saved sign-ins",
  "favicons.sqlite": "Site icons", "formhistory.sqlite": "Form history", cache2: "Cache",
  storage: "Website storage", "sessionstore.jsonlz4": "Session data",
};

function sizeLabel(value: unknown): string | null {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) return null;
  return value >= 1024 ? `${(value / 1024).toFixed(1)} MB` : `${value.toFixed(value < 100 ? 1 : 0)} KB`;
}

function visibleText(value: unknown): string | null {
  return typeof value === "string" ? (value === "" ? "(empty)" : value) : null;
}

function activityEntry(section: unknown, entry: RecordObject): RecordObject {
  if (section !== "WordWheelQuery" || !Array.isArray(entry.value) ||
      !entry.value.every(value => Number.isInteger(value) && value >= 0 && value <= 255)) return entry;
  const bytes = Uint8Array.from(entry.value as number[]);
  const rawBytes = Array.from(bytes, value => value.toString(16).padStart(2, "0")).join(" ");
  if (typeof entry.key === "string" && /^\d+$/.test(entry.key) && bytes.length % 2 === 0) {
    try {
      const text = new TextDecoder("utf-16le", { fatal: true }).decode(bytes).replace(/\0+$/, "");
      if (text && !/[\x00-\x08\x0b\x0c\x0e-\x1f]/.test(text)) return { ...entry, value: text, rawBytes };
    } catch { /* Preserve the bytes when Windows returned invalid text. */ }
  }
  return { ...entry, value: rawBytes };
}

function metadata(row: RecordObject, categoryId: string): string[] {
  const facts: string[] = [];
  const used = new Set<string>();
  const values = new Map(Object.entries(row).map(([key, value]) => [key.toLowerCase(), value]));
  const field = (key: string) => values.get(key.toLowerCase());
  const fields: Array<[string[], string]> = [
    [["name", "friendlyName", "processName", "displayName", "title"], "Name"],
    [["fileName"], "File"],
    [["path", "fullPath", "profilePath", "displayRoot", "deviceObject", "dir"], "Location"],
    [["originalPath"], "Original location"],
    [["target"], "Target"],
    [["source"], "Source"],
    [["destination"], "Destination"],
    [["command"], "Command"],
    [["value", "entry", "data"], "Value"],
    [["host", "hostname", "address", "domain", "url"], "Host"],
    [["username", "account"], "Account"],
    [["userSid", "sid"], "Account ID"],
    [["deviceId", "journalId", "id"], "Identifier"],
    [["interfaceId"], "Adapter"],
    [["drive"], "Drive"],
    [["maxSize"], "Maximum size"],
    [["originatingMachine"], "Originating machine"],
    [["serviceMachine"], "Service machine"],
    [["manufacturer"], "Manufacturer"],
    [["className"], "Class"],
    [["browser"], "Browser"],
    [["recordType", "type", "extension"], "Type"],
    [["status", "state", "stateStr"], "Status"],
    [["description"], "Description"],
    [["dnsSuffix"], "DNS suffix"],
    [["category"], "Category"],
    [["section"], "Section"],
    [["signed"], "Signature"],
    [["signer"], "Signer"],
    [["elevated"], "Elevated"],
  ];
  for (const [keys, label] of fields) {
    for (const key of keys) {
      const value = visibleText(field(key));
      if (value) { facts.push(`${label}: ${value}`); used.add(key.toLowerCase()); }
    }
  }
  if (["rdpHistory", "connectivityHistory", "ntUserTraces"].includes(categoryId)) {
    const key = visibleText(field("key"));
    if (key) { facts.push(`Key: ${key}`); used.add("key"); }
  }
  const sizeBytes = field("sizeBytes");
  const bytes = typeof sizeBytes === "number" ? sizeBytes / 1024 : undefined;
  const sizeMB = field("sizeMB");
  const mb = typeof sizeMB === "number" ? sizeMB * 1024 : undefined;
  const kb = sizeLabel(field("sizeKB") ?? bytes ?? mb);
  if (kb) { facts.push(`Size: ${kb}`); for (const key of ["sizekb", "sizebytes", "sizemb"]) if (values.has(key)) used.add(key); }
  for (const [key, label] of [["pid", "PID"], ["count", "Count"], ["ttl", "TTL"], ["recordTypeRaw", "Type code"], ["dataLength", "Data length"], ["timeStamp", "Timestamp"]] as const) {
    const value = field(key);
    if (typeof value === "number" && Number.isFinite(value) && value >= 0) { facts.push(`${label}: ${value}`); used.add(key.toLowerCase()); }
  }
  for (const [key, label] of [["present", "Present"], ["persistent", "Persistent"], ["clientAccessible", "Client accessible"]] as const) {
    const value = field(key);
    if (typeof value === "boolean") { facts.push(`${label}: ${value ? "Yes" : "No"}`); used.add(key.toLowerCase()); }
  }
  for (const [key, label] of [["lastRun", "Last run"], ["lastModified", "Last modified"], ["deletedTime", "Deleted"], ["modified", "Modified"], ["lastSeen", "Last seen"], ["created", "Created"], ["timestamp", "Date"]] as const) {
    const value = field(key);
    if (typeof value === "string" && value.trim()) {
      const display = /^\d{4}-\d{2}-\d{2}/.test(value) && Number.isFinite(Date.parse(value)) ? new Date(value).toLocaleString() : value;
      facts.push(`${label}: ${display}`); used.add(key.toLowerCase());
    }
  }
  // Include every remaining value returned for this record, including new
  // producer fields, without relying on a fixed field allowlist.
  const append = (key: string, value: unknown) => {
    if (value === null || value === undefined) return;
    const label = key.replace(/([a-z0-9])([A-Z])/g, "$1 $2").replace(/^./, c => c.toUpperCase());
    if (isObject(value)) {
      for (const [child, childValue] of Object.entries(value)) append(`${label} ${child}`, childValue);
    } else if (Array.isArray(value)) {
      for (const [index, child] of value.entries()) append(`${label} ${index + 1}`, child);
    } else if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
      facts.push(`${label}: ${typeof value === "boolean" ? (value ? "Yes" : "No") : value === "" ? "(empty)" : value}`);
    }
  };
  for (const [key, value] of Object.entries(row)) if (!used.has(key.toLowerCase())) append(key, value);
  return facts;
}

export function getSafeCleanupPreview(id: string, input: unknown): PreviewRow[] {
  if (!isObject(input)) return [];
  if (id === "amcache" && Array.isArray(input.entries)) {
    return input.entries.filter(isObject).flatMap((category) => {
      if (!Array.isArray(category.sample)) return [];
      const categoryName = typeof category.category === "string" ? amcacheLabels[category.category] ?? "Record" : "Record";
      const { category: sourceCategory, count: _count, sample: _sample, ...categoryDetails } = category;
      return category.sample.filter(isObject).map((sample, index) => ({
        label: `${categoryName} ${index + 1}`,
        facts: [...(typeof sourceCategory === "string" ? [`Category source: ${sourceCategory}`] : []), ...metadata(categoryDetails, id), ...metadata(sample, id)],
      }));
    });
  }
  const activitySections = Array.isArray(input.sections) ? input.sections : input.records;
  if (id === "ntUserTraces" && Array.isArray(activitySections) && activitySections.some(section => isObject(section) && Array.isArray(section.entries))) {
    const sectionNames: Record<string, string> = { RunMRU: "Run command", TypedPaths: "Typed path", TypedURLs: "Typed web address", WordWheelQuery: "Search entry" };
    return activitySections.flatMap((section, sectionIndex) => {
      // A partial response can contain both grouped sections and individual
      // observations. Preserve the latter even when other groups have entries.
      if (!isObject(section) || !Array.isArray(section.entries)) {
        const value = visibleText(section);
        return [{ label: `Activity record ${sectionIndex + 1}`, facts: isObject(section) ? metadata(section, id) : value ? [`Value: ${value}`] : [] }];
      }
      const label = typeof section.name === "string" ? sectionNames[section.name] ?? "Activity record" : "Activity record";
      const { name: sourceSection, count: _count, entries: _entries, ...sectionDetails } = section;
      return section.entries.map((entry, index) => ({ label: `${label} ${index + 1}`, facts: [...(typeof sourceSection === "string" ? [`Section source: ${sourceSection}`] : []), ...metadata(sectionDetails, id), ...(isObject(entry) ? metadata(activityEntry(section.name, entry), id) : (visibleText(entry) ? [`Value: ${visibleText(entry)}`] : []))] }));
    });
  }
  if (id === "wlanProfiles" && (Array.isArray(input.profiles) || Array.isArray(input.additionalProfiles))) {
    return wifiProfiles(input).map((profile, index) => ({ label: `Saved Wi-Fi profile ${index + 1}`, facts: isObject(profile) ? metadata(profile, id) : [] }));
  }
  if (id === "clipboardHistory" && Array.isArray(input.historyItems)) {
    return input.historyItems.map((item, index) => ({ label: `Clipboard item ${index + 1}`, facts: isObject(item) ? metadata(item, id) : [] }));
  }
  if (id === "netDrives" && Array.isArray(input.drives)) {
    return input.drives.map((value, index) => {
      const candidate = isObject(value) ? value.Name ?? value.localName : null;
      const drive = typeof candidate === "string" && /^[A-Za-z]:?$/.test(candidate.trim()) ? candidate.trim().replace(/:?$/, ":").toUpperCase() : null;
      return { label: drive ? `Drive ${drive}` : `Network drive ${index + 1}`, facts: isObject(value) ? metadata(value, id) : [] };
    });
  }
  if (id === "recycleBin" && Array.isArray(input.items)) {
    return input.items.map((item, index) => ({ label: `Recycle Bin record ${index + 1}`, facts: isObject(item) ? metadata(item, id) : [] }));
  }
  const browsers = Array.isArray(input.browsers) ? input.browsers : input.records;
  if (id === "browserFootprints" && Array.isArray(browsers)) {
    return browsers.filter(isObject).flatMap((browser, index) => {
      const name = typeof browser.browser === "string" ? browserNames[browser.browser.trim().toLowerCase()] : undefined;
      const size = sizeLabel(browser.totalSizeKB);
      const { artifacts: browserArtifacts, ...browserDetails } = browser;
      const artifacts = Array.isArray(browserArtifacts) ? browserArtifacts.filter(isObject).flatMap((artifact) => {
        const label = (typeof artifact.name === "string" ? artifactNames[artifact.name.trim().toLowerCase()] ?? visibleText(artifact.name) : undefined) ?? "Unnamed artifact";
        const artifactSize = sizeLabel(artifact.sizeKB);
        const profilePath = visibleText(artifact.profilePath);
        const rawName = artifact.name;
        const artifactDetails = { ...artifact };
        delete artifactDetails.name; delete artifactDetails.profilePath; delete artifactDetails.sizeKB;
        return [{ label: name ?? `Browser ${index + 1}`, facts: [...metadata(browserDetails, id), ...(size ? [`Total size: ${size}`] : []), `Artifact: ${label}`, ...(typeof rawName === "string" && rawName !== label ? [`Artifact source: ${rawName}`] : []), ...(profilePath ? [`Profile: ${profilePath}`] : []), ...(artifactSize ? [`Artifact size: ${artifactSize}`] : []), ...metadata(artifactDetails, id)] }];
      }) : [];
      return artifacts.length ? artifacts : [{ label: name ?? `Browser ${index + 1}`, facts: [...metadata(browserDetails, id), ...(size ? [`Total size: ${size}`] : [])] }];
    });
  }
  // Multiple scan collections can contain independent records. Keep each
  // collection rather than stopping after the first populated array.
  const collections = ["records", "entries", "files", "logs", "devices", "journals", "copies", "dumps", "databases", "processes", "historyItems", "drives", "items", "paths"]
    .flatMap(key => Array.isArray(input[key]) && input[key].length ? [{ key, values: input[key] as unknown[] }] : []);
  if (collections.length) return collections.flatMap(({ key, values }) => values.map((value, index) => {
    const text = visibleText(value);
    const facts = isObject(value) ? metadata(value, id) : text ? [text] : [];
    if (collections.length > 1) facts.unshift(`Dataset: ${key}`);
    return { label: `Record ${index + 1}`, facts };
  }));
  return [];
}

export function shouldLoadCurrentUserDetails(categoryId: string, accountLabel?: string): boolean {
  return !accountLabel && (categoryId === "wlanProfiles" || categoryId === "clipboardHistory");
}

export function wifiPasswordForProfile(profile: unknown, secrets: Array<{ name: string; interfaceId?: string; password?: string | null; passwordStatus?: string }> | null, loading: boolean, accountLabel?: string): string {
  if (secrets && isObject(profile) && typeof profile.name === "string") {
    const matches = secrets.filter(secret => secret.name === profile.name &&
      (typeof profile.interfaceId !== "string" || secret.interfaceId?.toLowerCase() === profile.interfaceId.toLowerCase()));
    if (matches.length === 1 && matches[0].passwordStatus === "available" && typeof matches[0].password === "string") return matches[0].password || "Empty password";
  }
  // A failed or ambiguous refresh does not erase a value already returned by
  // the scan. It may be older than a successful refresh, which takes priority.
  if (isObject(profile) && typeof profile.password === "string") return profile.password || "Empty password";
  return accountLabel ? "Unavailable for this account" : loading ? "Loading…" : "Unavailable";
}

type CleanupRecordChunk = { offset: number; totalBytes: number; data: string; nextByteOffset: number | null };

export function decodeCleanupRecordChunk(chunk: CleanupRecordChunk, expectedOffset: number): Uint8Array | null {
  if (!chunk || !Number.isSafeInteger(chunk.offset) || chunk.offset !== expectedOffset || !Number.isSafeInteger(chunk.totalBytes) || chunk.totalBytes <= expectedOffset ||
      typeof chunk.data !== "string" || chunk.data.length > 4194304 || !/^[A-Za-z0-9+/]*={0,2}$/.test(chunk.data)) return null;
  try {
    const binary = atob(chunk.data);
    if (!binary.length || binary.length > 3145728 || btoa(binary) !== chunk.data) return null;
    const end = expectedOffset + binary.length;
    if (end > chunk.totalBytes || (chunk.nextByteOffset === null ? end !== chunk.totalBytes : chunk.nextByteOffset !== end || end >= chunk.totalBytes)) return null;
    const bytes = new Uint8Array(binary.length);
    for (let index = 0; index < binary.length; index++) bytes[index] = binary.charCodeAt(index);
    return bytes;
  } catch { return null; }
}

export function assembleCleanupRecordChunks(parts: Uint8Array[], totalBytes: number): RecordObject | null {
  if (!Number.isSafeInteger(totalBytes) || totalBytes <= 0 || parts.reduce((sum, part) => sum + part.length, 0) !== totalBytes) return null;
  try {
    const bytes = new Uint8Array(totalBytes);
    let position = 0;
    for (const part of parts) { bytes.set(part, position); position += part.length; }
    const record: unknown = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
    return isObject(record) ? record : null;
  } catch { return null; }
}

export function readExpandedCleanupSummary(data: { error?: boolean; users?: Array<{ username: string; categories: Array<{ id: string; count: number; items: string[]; records?: Array<Record<string, unknown>>; sampleCount?: number; previewLimit?: number; truncated?: boolean; sourceTruncated?: boolean; recordChunk?: CleanupRecordChunk; snapshotHash?: string; nextOffset?: number | null; totalRows?: number; status?: string; partial?: boolean; complete?: boolean; message?: string; error?: boolean }> }> } | undefined, accountName: string, categoryId: string, offset = 0, expectedHash = "", chunkOffset = 0) {
  const users = data?.users;
  const user = Array.isArray(users) && users.length === 1 ? users[0] : undefined;
  const matching = user && typeof user.username === "string" && user.username.toLocaleLowerCase() === accountName.toLocaleLowerCase() && Array.isArray(user.categories)
    ? user.categories.filter(category => category.id === categoryId) : [];
  if (data?.error || matching.length !== 1) return null;
  const category = matching[0];
  const pageCount = Array.isArray(category.records) ? category.records.length : -1;
  const chunkBytes = category.recordChunk ? decodeCleanupRecordChunk(category.recordChunk, chunkOffset) : null;
  const recordCount = category.recordChunk ? 1 : pageCount;
  if (typeof category.snapshotHash !== "string" || !/^sha256:[0-9a-f]{64}$/.test(category.snapshotHash) || (expectedHash && category.snapshotHash !== expectedHash) ||
      !Number.isSafeInteger(category.totalRows) || (category.totalRows as number) < 0 || pageCount < 0 || pageCount > 5000 ||
      (category.recordChunk ? pageCount !== 0 || !chunkBytes || typeof category.sourceTruncated !== "boolean" : chunkOffset !== 0) ||
      offset + recordCount > (category.totalRows as number) ||
      (category.nextOffset === null ? offset + recordCount !== category.totalRows : !Number.isSafeInteger(category.nextOffset) || category.nextOffset !== offset + recordCount || (category.nextOffset as number) >= (category.totalRows as number) || recordCount === 0)) return null;
  // A page may contain only some activity sections. Their combined count must
  // be checked against the scan total after the final page is assembled.
  let pageCategory = category;
  const groupedActivity = categoryId === "ntUserTraces" && !!category.records?.some(row => isObject(row) && Object.hasOwn(row, "count"));
  if (groupedActivity) {
    if (!Number.isSafeInteger(category.count) || category.count < 0 || !category.records?.every(row => isObject(row) && Number.isSafeInteger(row.count) && (row.count as number) >= 0)) return null;
    const pageTotal = category.records.reduce((sum, row) => sum + (row.count as number), 0);
    if (!Number.isSafeInteger(pageTotal) || pageTotal > category.count) return null;
    pageCategory = { ...category, count: pageTotal };
  }
  // A partial source can still return snapshot-verified pages of observations.
  // Validate the page structure independently without accepting its total as a
  // complete scan; the outer card remains failed and cannot authorize Clear.
  const partial = category.partial === true || category.complete === false || category.status === "partial";
  if (category.error || (category.status && ["error", "failed", "unknown", "unavailable"].includes(category.status))) return null;
  const summary = readCleanupUserSummary(partial ? { ...pageCategory, partial: false, complete: true, status: "scoped" } : pageCategory);
  if (summary.count < 0 || !summary.raw) return null;
  return { ...summary, count: category.count, partial, raw: { ...summary.raw, ...(partial ? { partial: true } : {}), ...(groupedActivity ? { expectedActivityCount: category.count } : {}) }, snapshotHash: category.snapshotHash, nextOffset: category.nextOffset, totalRows: category.totalRows as number, recordChunk: category.recordChunk, chunkBytes, sourceTruncated: category.sourceTruncated };
}

export function mergeExpandedCleanupPage(previousRaw: unknown, currentRaw: unknown, offset: number): RecordObject | null {
  const previous = isObject(previousRaw) && Array.isArray(previousRaw.records) ? previousRaw.records : [];
  if (!isObject(currentRaw) || !Array.isArray(currentRaw.records) || previous.length !== offset) return null;
  const records = [...previous, ...currentRaw.records];
  if (currentRaw.truncated === false && Number.isSafeInteger(currentRaw.expectedActivityCount)) {
    if (!records.every(row => isObject(row) && Number.isSafeInteger(row.count) && (row.count as number) >= 0)) return null;
    const total = records.reduce((sum, row) => sum + (row.count as number), 0);
    if (!Number.isSafeInteger(total) || total !== currentRaw.expectedActivityCount) return null;
  }
  return { ...currentRaw, records, sampleCount: records.length };
}

export default function CleanupRecordPreview({ categoryId, rawData, mode, accountLabel, accountName }: { categoryId: string; rawData: unknown; mode: CleanupDisplayMode; accountLabel?: string; accountName?: string }) {
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(0);
  const [sortDescending, setSortDescending] = useState(false);
  const backend = useBackend();
  const backendRef = useRef(backend);
  backendRef.current = backend;
  const [secrets, setSecrets] = useState<Array<{ name: string; interfaceId?: string; password?: string | null; passwordStatus?: string }> | null>(null);
  const [secretError, setSecretError] = useState<string | null>(null);
  const [clipboardItems, setClipboardItems] = useState<Array<{ type: string; preview: string; charCount: number }> | null>(null);
  const [clipboardNotice, setClipboardNotice] = useState<string | null>(null);
  const [loadingSecrets, setLoadingSecrets] = useState(false);
  const [expandedRaw, setExpandedRaw] = useState<unknown>(null);
  const [expandedNotice, setExpandedNotice] = useState<string | null>(null);
  const [loadingExpanded, setLoadingExpanded] = useState(false);
  const [nextPage, setNextPage] = useState<{ offset: number; hash: string; totalRows: number; count: number; partial: boolean } | null>(null);
  const requestVersion = useRef(0);
  useEffect(() => {
    let active = true;
    setSecrets(null);
    setClipboardItems(null);
    setClipboardNotice(null);
    setSecretError(null);
    setLoadingSecrets(false);
    setExpandedRaw(null);
    setExpandedNotice(null);
    setLoadingExpanded(false);
    setNextPage(null);
    if (categoryId === "wlanProfiles" && shouldLoadCurrentUserDetails(categoryId, accountLabel)) {
      setLoadingSecrets(true);
      void backendRef.current.getWlanProfileSecrets().then(result => {
        if (!active) return;
        if (result.success && result.data && Array.isArray(result.data.profiles)) setSecrets([...result.data.profiles, ...(result.data.additionalProfiles ?? [])]);
        else setSecretError(result.error || "Passwords could not be retrieved.");
      }).catch(() => { if (active) setSecretError("Passwords could not be retrieved."); })
        .finally(() => { if (active) setLoadingSecrets(false); });
    }
    if (categoryId === "clipboardHistory" && shouldLoadCurrentUserDetails(categoryId, accountLabel)) {
      void backendRef.current.getClipboardHistoryStatus(true).then(result => {
        if (!active) return;
        if (result.success && result.data && Array.isArray(result.data.historyItems)) {
          setClipboardItems(result.data.historyItems);
          if (result.data.partial || result.data.historyReadStatus !== "complete") setClipboardNotice(result.data.message || "Some clipboard details may be unavailable.");
        } else setClipboardNotice(result.error || "Clipboard details could not be retrieved.");
      }).catch(() => { if (active) setClipboardNotice("Clipboard details could not be retrieved."); });
    }
    return () => { active = false; requestVersion.current += 1; };
  }, [categoryId, rawData, accountLabel, accountName]);
  const loadFullDetails = async () => {
    if (!accountName) return;
    const request = ++requestVersion.current;
    setLoadingExpanded(true);
    setExpandedNotice(null);
    try {
      const offset = nextPage?.offset ?? 0;
      const result = await backendRef.current.getCleanupSummaryAllUsers([categoryId], [accountName], true, offset, nextPage?.hash ?? "");
      if (request !== requestVersion.current) return;
      let summary = result.success ? readExpandedCleanupSummary(result.data, accountName, categoryId, offset, nextPage?.hash ?? "") : null;
      if (!summary || (nextPage && (summary.count !== nextPage.count || summary.totalRows !== nextPage.totalRows || summary.partial !== nextPage.partial))) {
        setExpandedNotice(result.error || result.data?.message || "Account details changed or could not be verified. Close and reopen to rescan.");
        return;
      }
      if (summary.recordChunk && summary.chunkBytes) {
        const first = summary;
        const firstChunk = summary.recordChunk;
        const parts = [summary.chunkBytes];
        let byteOffset = firstChunk.nextByteOffset;
        while (byteOffset !== null) {
          setExpandedNotice(`Loading a large record: ${byteOffset} of ${firstChunk.totalBytes} bytes.`);
          const continuation = await backendRef.current.getCleanupSummaryAllUsers([categoryId], [accountName], true, offset, first.snapshotHash, byteOffset);
          if (request !== requestVersion.current) return;
          const next = continuation.success ? readExpandedCleanupSummary(continuation.data, accountName, categoryId, offset, first.snapshotHash, byteOffset) : null;
          if (!next?.recordChunk || !next.chunkBytes || next.recordChunk.totalBytes !== firstChunk.totalBytes ||
              next.nextOffset !== first.nextOffset || next.totalRows !== first.totalRows || next.count !== first.count || next.partial !== first.partial || next.sourceTruncated !== first.sourceTruncated) {
            setExpandedNotice("Account details changed or the large record could not be verified. Close and reopen to rescan.");
            return;
          }
          parts.push(next.chunkBytes);
          byteOffset = next.recordChunk.nextByteOffset;
        }
        const record = assembleCleanupRecordChunks(parts, firstChunk.totalBytes);
        if (!record || !isObject(first.raw)) throw new Error("Invalid account detail record.");
        summary = { ...first, raw: { ...first.raw, records: [record], sampleCount: 1, truncated: first.sourceTruncated || first.nextOffset !== null, ...(categoryId === "ntUserTraces" && Object.hasOwn(record, "count") ? { expectedActivityCount: first.count } : {}) } };
      }
      const merged = mergeExpandedCleanupPage(expandedRaw, summary.raw, offset);
      if (!merged) { setExpandedNotice("Account details changed. Close and reopen to rescan."); return; }
      setExpandedRaw(merged);
      setNextPage(typeof summary.nextOffset === "number" ? { offset: summary.nextOffset, hash: summary.snapshotHash, totalRows: summary.totalRows, count: summary.count, partial: summary.partial } : null);
      setExpandedNotice(summary.partial ? "More observed details loaded. The scan remains incomplete." : `Refreshed account scan: ${summary.count} items reported.`);
      if (offset === 0) setPage(0);
    } catch {
      if (request === requestVersion.current) setExpandedNotice("Full details could not be read.");
    } finally {
      if (request === requestVersion.current) setLoadingExpanded(false);
    }
  };
  const shownData = expandedRaw ?? rawData;
  const all = useMemo(() => clipboardItems ? clipboardItems.map((item, index) => ({
    label: `Clipboard item ${index + 1}`,
    facts: metadata(item, categoryId),
  })) : getSafeCleanupPreview(categoryId, shownData), [categoryId, shownData, clipboardItems]);
  const filtered = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    return all.map((row, index) => ({ ...row, number: index + 1 }))
      .filter(row => `${row.label} ${row.facts.join(" ")}`.toLocaleLowerCase().includes(needle))
      .sort((a, b) => (sortDescending ? -1 : 1) * (a.number - b.number));
  }, [all, query, sortDescending]);
  const columns = useMemo(() => {
    const labels = new Set<string>();
    if (categoryId === "amcache" || categoryId === "ntUserTraces") labels.add("Category");
    for (const row of all) for (const fact of row.facts) {
      const separator = fact.indexOf(": ");
      labels.add(separator < 0 ? "Details" : fact.slice(0, separator));
    }
    if (categoryId === "wlanProfiles") labels.add("Password");
    if (!labels.size) labels.add("Details");
    return [...labels];
  }, [all, categoryId]);
  if (!all.length && !clipboardItems && !(isObject(shownData) && shownData.truncated === true && accountName)) return null;
  const pageSize = mode === "simple" ? 5 : 10;
  const currentPage = Math.min(page, Math.max(0, Math.ceil(filtered.length / pageSize) - 1));
  const rows = filtered.slice(currentPage * pageSize, (currentPage + 1) * pageSize);
  return <section className="cleanup-record-preview" aria-label="Found details">
    <div className="cleanup-record-preview__header">
      <div><h3>Found details</h3><p>{all.length} {all.length === 1 ? "item" : "items"} available; the scan total is above.</p></div>
      <label><span className="sr-only">Filter found details</span><input type="search" value={query} placeholder="Filter details" aria-label={accountLabel ? `Filter ${accountLabel} details` : "Filter details"} onChange={event => { setQuery(event.currentTarget.value); setPage(0); }} /></label>
    </div>
    {categoryId === "wlanProfiles" && isObject(shownData) && Array.isArray(shownData.additionalProfiles) && shownData.additionalProfiles.length > 0 ? <p>Additional per-user or policy-managed profiles are shown for reference. The scan total and Clear action cover only all-user profiles.</p> : null}
    {isObject(shownData) && shownData.truncated === true ? <p role="status">Only part of this scan is shown; {accountName && (!expandedRaw || nextPage) ? "load more account details to expand this preview." : "additional records were not returned."}</p> : null}
    {isObject(shownData) && shownData.truncated === true && accountName && (!expandedRaw || nextPage) ? <button type="button" disabled={loadingExpanded} onClick={loadFullDetails}>{loadingExpanded ? "Loading account details…" : "Load more account details"}</button> : null}
    {expandedNotice ? <p role="status">{expandedNotice}</p> : null}
    {clipboardNotice ? <p role="status">{clipboardNotice}</p> : null}
    {secretError ? <p role="alert">{secretError}</p> : null}
    <div className="cleanup-readable-detail__table cleanup-record-preview__table"><table>
      <thead><tr><th scope="col" aria-sort={sortDescending ? "descending" : "ascending"}><button type="button" onClick={() => setSortDescending(value => !value)} aria-label={`Sort items ${sortDescending ? "ascending" : "descending"}`}># {sortDescending ? "↓" : "↑"}</button></th>{columns.map(label => <th scope="col" key={label}>{label}</th>)}</tr></thead>
      <tbody>{rows.map(row => {
        const cells = new Map<string, string[]>();
        if (categoryId === "amcache" || categoryId === "ntUserTraces") cells.set("Category", [row.label.replace(/ \d+$/, "")]);
        if (categoryId === "wlanProfiles") {
          const profiles = wifiProfiles(shownData);
          const profile = profiles[row.number - 1];
          cells.set("Password", [wifiPasswordForProfile(profile, secrets, loadingSecrets, accountLabel)]);
        }
        for (const fact of row.facts) {
          const separator = fact.indexOf(": ");
          const label = separator < 0 ? "Details" : fact.slice(0, separator);
          const value = separator < 0 ? fact : fact.slice(separator + 2);
          if (categoryId !== "wlanProfiles" || label !== "Password" || !cells.has(label)) cells.set(label, [...(cells.get(label) ?? []), value]);
        }
        return <tr key={row.number}><td>{row.number}</td>{columns.map(label => <td key={label}>{cells.get(label)?.join("; ") || "—"}</td>)}</tr>;
      })}</tbody>
    </table></div>
    {!rows.length ? <p role="status">No items match this filter.</p> : null}
    {filtered.length > pageSize ? <nav className="cleanup-record-preview__pagination" aria-label="Detail pages"><span>{currentPage * pageSize + 1}–{currentPage * pageSize + rows.length} of {filtered.length} items</span><div><button type="button" disabled={currentPage === 0} onClick={() => setPage(currentPage - 1)}>Previous</button><button type="button" disabled={(currentPage + 1) * pageSize >= filtered.length} onClick={() => setPage(currentPage + 1)}>Next</button></div></nav> : null}
  </section>;
}
