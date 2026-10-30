import type { CleanupDisplayMode } from "../../panels/cleanup/cleanupDisplay";
import { useMemo, useState } from "react";

type RecordObject = Record<string, unknown>;
type PreviewRow = { label: string; facts: string[] };
const isObject = (value: unknown): value is RecordObject => value !== null && typeof value === "object" && !Array.isArray(value);

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

function visibleText(value: unknown, redactSecrets = false): string | null {
  if (typeof value !== "string" || !value.trim()) return null;
  // History commands can contain credentials even when the field is named "command".
  if (redactSecrets && /\b(password|passwd|passphrase|secret|token|credential|api[_-]?key)(?:\s|[:=])/i.test(value)) return "Sensitive value hidden";
  return value.replace(/(https?:\/\/)[^@\s/]+@/gi, "$1[redacted]@");
}

function metadata(row: RecordObject): string[] {
  const facts: string[] = [];
  const values = new Map(Object.entries(row).map(([key, value]) => [key.toLowerCase(), value]));
  const field = (key: string) => values.get(key.toLowerCase());
  const fields: Array<[string[], string]> = [
    [["name", "friendlyName", "processName", "displayName", "title"], "Name"],
    [["fileName"], "File"],
    [["path", "fullPath", "profilePath", "originalPath", "target", "displayRoot", "deviceObject", "dir"], "Location"],
    [["source"], "Source"],
    [["destination"], "Destination"],
    [["command"], "Command"],
    [["value", "entry", "data"], "Value"],
    [["host", "hostname", "address", "domain", "url"], "Host"],
    [["username", "account", "userSid"], "Account"],
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
    [["signed"], "Signature"],
    [["signer"], "Signer"],
    [["elevated"], "Elevated"],
  ];
  for (const [keys, label] of fields) {
    const value = keys.map(key => visibleText(field(key), ["Command", "Value", "Description"].includes(label))).find(Boolean);
    if (value) facts.push(`${label}: ${value}`);
  }
  const sizeBytes = field("sizeBytes");
  const bytes = typeof sizeBytes === "number" ? sizeBytes / 1024 : undefined;
  const sizeMB = field("sizeMB");
  const mb = typeof sizeMB === "number" ? sizeMB * 1024 : undefined;
  const kb = sizeLabel(field("sizeKB") ?? bytes ?? mb);
  if (kb) facts.push(`Size: ${kb}`);
  for (const [key, label] of [["pid", "PID"], ["count", "Count"], ["ttl", "TTL"]] as const) {
    const value = field(key);
    if (typeof value === "number" && Number.isFinite(value) && value >= 0) facts.push(`${label}: ${value}`);
  }
  for (const [key, label] of [["present", "Present"], ["persistent", "Persistent"], ["clientAccessible", "Client accessible"]] as const) {
    const value = field(key);
    if (typeof value === "boolean") facts.push(`${label}: ${value ? "Yes" : "No"}`);
  }
  for (const key of ["lastRun", "lastModified", "deletedTime", "modified", "lastSeen", "created", "timestamp"]) {
    const value = field(key);
    if (typeof value === "string" && /^\d{4}-\d{2}-\d{2}/.test(value) && Number.isFinite(Date.parse(value))) {
      facts.push(`Date: ${new Date(value).toLocaleString()}`); break;
    }
  }
  return facts;
}

export function getSafeCleanupPreview(id: string, input: unknown): PreviewRow[] {
  if (!isObject(input)) return [];
  if (id === "amcache" && Array.isArray(input.entries)) {
    return input.entries.filter(isObject).flatMap((category) => {
      if (!Array.isArray(category.sample)) return [];
      const categoryName = typeof category.category === "string" ? amcacheLabels[category.category] ?? "Record" : "Record";
      return category.sample.filter(isObject).map((sample, index) => ({
        label: `${categoryName} ${index + 1}`,
        facts: metadata(sample),
      }));
    });
  }
  const activitySections = Array.isArray(input.sections) ? input.sections : input.records;
  if (id === "ntUserTraces" && Array.isArray(activitySections) && activitySections.some(section => isObject(section) && Array.isArray(section.entries))) {
    const sectionNames: Record<string, string> = { RunMRU: "Run command", TypedPaths: "Typed path", TypedURLs: "Typed web address", WordWheelQuery: "Search entry" };
    return activitySections.filter(isObject).flatMap((section) => {
      if (!Array.isArray(section.entries)) return [];
      const label = typeof section.name === "string" ? sectionNames[section.name] ?? "Activity record" : "Activity record";
      return section.entries.map((entry, index) => ({ label: `${label} ${index + 1}`, facts: isObject(entry) ? metadata(entry) : [] }));
    });
  }
  if (id === "wlanProfiles" && Array.isArray(input.profiles)) {
    return input.profiles.map((profile, index) => ({ label: `Saved Wi-Fi profile ${index + 1}`, facts: isObject(profile) ? metadata(profile) : [] }));
  }
  if (id === "clipboardHistory" && Array.isArray(input.historyItems)) {
    return input.historyItems.map((_, index) => ({ label: `Clipboard item ${index + 1}`, facts: ["Content not supplied by the scan"] }));
  }
  if (id === "netDrives" && Array.isArray(input.drives)) {
    return input.drives.map((value, index) => {
      const candidate = isObject(value) ? value.Name ?? value.localName : null;
      const drive = typeof candidate === "string" && /^[A-Za-z]:?$/.test(candidate.trim()) ? candidate.trim().replace(/:?$/, ":").toUpperCase() : null;
      return { label: drive ? `Drive ${drive}` : `Network drive ${index + 1}`, facts: isObject(value) ? metadata(value) : [] };
    });
  }
  if (id === "recycleBin" && Array.isArray(input.items)) {
    return input.items.map((item, index) => ({ label: `Recycle Bin record ${index + 1}`, facts: isObject(item) ? metadata(item) : [] }));
  }
  const browsers = Array.isArray(input.browsers) ? input.browsers : input.records;
  if (id === "browserFootprints" && Array.isArray(browsers)) {
    return browsers.filter(isObject).map((browser, index) => {
      const name = typeof browser.browser === "string" ? browserNames[browser.browser.trim().toLowerCase()] : undefined;
      const size = sizeLabel(browser.totalSizeKB);
      const artifacts = Array.isArray(browser.artifacts) ? browser.artifacts.filter(isObject).flatMap((artifact) => {
        const label = typeof artifact.name === "string" ? artifactNames[artifact.name.trim().toLowerCase()] ?? visibleText(artifact.name) : undefined;
        if (!label) return [];
        const artifactSize = sizeLabel(artifact.sizeKB);
        return [artifactSize ? `${label}: ${artifactSize}` : label];
      }) : [];
      return { label: name ?? `Browser ${index + 1}`, facts: [...metadata(browser), ...(size ? [`Total size: ${size}`] : []), ...artifacts] };
    });
  }
  // Show known record fields without exposing arbitrary credential fields.
  for (const key of ["records", "entries", "files", "logs", "devices", "journals", "copies", "dumps", "databases", "processes", "historyItems", "drives", "items", "paths"]) {
    const values = input[key];
    if (!Array.isArray(values) || !values.length) continue;
    return values.map((value, index) => {
      const text = visibleText(value, true);
      return { label: `Record ${index + 1}`, facts: isObject(value) ? metadata(value) : text ? [text] : [] };
    });
  }
  return [];
}

export default function CleanupRecordPreview({ categoryId, rawData, mode, accountLabel }: { categoryId: string; rawData: unknown; mode: CleanupDisplayMode; accountLabel?: string }) {
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(0);
  const [sortDescending, setSortDescending] = useState(false);
  const all = useMemo(() => getSafeCleanupPreview(categoryId, rawData), [categoryId, rawData]);
  const filtered = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    return all.filter(row => `${row.label} ${row.facts.join(" ")}`.toLocaleLowerCase().includes(needle))
      .sort((a, b) => (sortDescending ? -1 : 1) * a.label.localeCompare(b.label, undefined, { numeric: true }));
  }, [all, query, sortDescending]);
  if (!all.length) return null;
  const pageSize = mode === "simple" ? 5 : 10;
  const currentPage = Math.min(page, Math.max(0, Math.ceil(filtered.length / pageSize) - 1));
  const rows = filtered.slice(currentPage * pageSize, (currentPage + 1) * pageSize);
  return <section className="cleanup-record-preview" aria-label="Record preview">
    <div className="cleanup-record-preview__header">
      <div><h3>Record preview</h3><p>{all.length} preview {all.length === 1 ? "row" : "rows"} available here; the scan total is above.</p></div>
      <label><span className="sr-only">Filter record preview</span><input type="search" value={query} placeholder="Filter preview" aria-label={accountLabel ? `Filter ${accountLabel} record preview` : "Filter record preview"} onChange={event => { setQuery(event.currentTarget.value); setPage(0); }} /></label>
    </div>
    <div className="cleanup-readable-detail__table"><table>
      <thead><tr><th scope="col" aria-sort={sortDescending ? "descending" : "ascending"}><button type="button" onClick={() => setSortDescending(value => !value)} aria-label={`Sort preview records ${sortDescending ? "ascending" : "descending"}`}>Record {sortDescending ? "↓" : "↑"}</button></th><th scope="col">{mode === "simple" ? "Details" : "Preview details"}</th></tr></thead>
      <tbody>{rows.map((row, index) => <tr key={`${row.label}:${index}`}><td>{row.label}</td><td>{row.facts.length ? row.facts.join(" · ") : "No displayable details returned"}</td></tr>)}</tbody>
    </table></div>
    {!rows.length ? <p role="status">No preview rows match this filter.</p> : null}
    {filtered.length > pageSize ? <nav className="cleanup-record-preview__pagination" aria-label="Record preview pages"><span>{currentPage * pageSize + 1}–{currentPage * pageSize + rows.length} of {filtered.length} preview rows</span><div><button type="button" disabled={currentPage === 0} onClick={() => setPage(currentPage - 1)}>Previous</button><button type="button" disabled={(currentPage + 1) * pageSize >= filtered.length} onClick={() => setPage(currentPage + 1)}>Next</button></div></nav> : null}
  </section>;
}
