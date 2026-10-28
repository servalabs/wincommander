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

function appName(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const name = value.trim();
  // A product name is useful metadata; paths, addresses and secret-shaped values are not.
  if (!name || name.length > 100 || /[\\/:@\r\n]/.test(name) || /password|secret|credential|token|api[ _-]?key/i.test(name)) return null;
  return name;
}

function metadata(row: RecordObject): string[] {
  const facts: string[] = [];
  const kb = sizeLabel(row.sizeKB ?? row.SizeKB ?? row.sizeKb);
  if (kb) facts.push(`Size: ${kb}`);
  for (const [key, label] of [["count", "Count"], ["Count", "Count"]] as const) {
    const value = row[key];
    if (typeof value === "number" && Number.isFinite(value) && value >= 0) { facts.push(`${label}: ${value}`); break; }
  }
  for (const key of ["lastRun", "lastModified", "lastSeen", "created", "timestamp"]) {
    const value = row[key];
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
      return category.sample.filter(isObject).slice(0, 20).map((sample, index) => ({
        label: `${categoryName} ${index + 1}`,
        // Application names are record metadata. Never show IDs, paths or file contents.
        facts: category.category === "InventoryApplication" || category.category === "Application"
          ? [appName(sample.name)].filter((name): name is string => name !== null).map((name) => `App: ${name}`)
          : [],
      }));
    }).slice(0, 20);
  }
  if (id === "ntUserTraces" && Array.isArray(input.sections)) {
    const sectionNames: Record<string, string> = { RunMRU: "Run command", TypedPaths: "Typed path", TypedURLs: "Typed web address", WordWheelQuery: "Search entry" };
    return input.sections.filter(isObject).flatMap((section) => {
      if (!Array.isArray(section.entries)) return [];
      const label = typeof section.name === "string" ? sectionNames[section.name] ?? "Activity record" : "Activity record";
      return section.entries.slice(0, 20).map((_, index) => ({ label: `${label} ${index + 1}`, facts: ["Private contents hidden"] }));
    }).slice(0, 20);
  }
  if (id === "wlanProfiles" && Array.isArray(input.profiles)) {
    return input.profiles.slice(0, 20).map((_, index) => ({ label: `Saved Wi-Fi profile ${index + 1}`, facts: ["Network name and password hidden"] }));
  }
  if (id === "clipboardHistory" && Array.isArray(input.historyItems)) {
    return input.historyItems.slice(0, 20).map((_, index) => ({ label: `Clipboard item ${index + 1}`, facts: ["Content hidden"] }));
  }
  if (id === "netDrives" && Array.isArray(input.drives)) {
    return input.drives.slice(0, 20).map((value, index) => {
      const candidate = isObject(value) ? value.Name ?? value.localName : null;
      const drive = typeof candidate === "string" && /^[A-Za-z]:?$/.test(candidate.trim()) ? candidate.trim().replace(/:?$/, ":").toUpperCase() : null;
      return { label: drive ? `Drive ${drive}` : `Network drive ${index + 1}`, facts: ["Network location hidden"] };
    });
  }
  if (id === "recycleBin" && Array.isArray(input.items)) {
    return input.items.slice(0, 20).map((_, index) => ({ label: `Recycle Bin record ${index + 1}`, facts: ["File details hidden"] }));
  }
  if (id === "browserFootprints" && Array.isArray(input.browsers)) {
    return input.browsers.filter(isObject).slice(0, 20).map((browser, index) => {
      const name = typeof browser.browser === "string" ? browserNames[browser.browser.trim().toLowerCase()] : undefined;
      const size = sizeLabel(browser.totalSizeKB);
      const artifacts = Array.isArray(browser.artifacts) ? browser.artifacts.filter(isObject).slice(0, 20).flatMap((artifact) => {
        const label = typeof artifact.name === "string" ? artifactNames[artifact.name.trim().toLowerCase()] : undefined;
        if (!label) return [];
        const artifactSize = sizeLabel(artifact.sizeKB);
        return [artifactSize ? `${label}: ${artifactSize}` : label];
      }) : [];
      return { label: name ?? `Browser ${index + 1}`, facts: [...(size ? [`Total size: ${size}`] : []), ...artifacts] };
    });
  }
  // For other sources, expose only bounded numeric/date metadata from known collections.
  for (const key of ["records", "entries", "files", "logs", "devices", "journals", "copies", "dumps", "databases", "processes", "historyItems", "drives", "items", "paths"]) {
    const values = input[key];
    if (!Array.isArray(values) || !values.length) continue;
    return values.slice(0, 20).map((value, index) => ({ label: `Record ${index + 1}`, facts: isObject(value) ? metadata(value) : [] }));
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
      <div><h3>Record preview</h3><p>{all.length} preview {all.length === 1 ? "row" : "rows"} available here{all.length === 20 ? " (up to 20 shown)" : ""}; the scan total is above.</p></div>
      <label><span className="sr-only">Filter record preview</span><input type="search" value={query} placeholder="Filter preview" aria-label={accountLabel ? `Filter ${accountLabel} record preview` : "Filter record preview"} onChange={event => { setQuery(event.currentTarget.value); setPage(0); }} /></label>
    </div>
    <div className="cleanup-readable-detail__table"><table>
      <thead><tr><th scope="col" aria-sort={sortDescending ? "descending" : "ascending"}><button type="button" onClick={() => setSortDescending(value => !value)} aria-label={`Sort preview records ${sortDescending ? "ascending" : "descending"}`}>Record {sortDescending ? "↓" : "↑"}</button></th><th scope="col">{mode === "simple" ? "Details" : "Preview details"}</th></tr></thead>
      <tbody>{rows.map((row, index) => <tr key={`${row.label}:${index}`}><td>{row.label}</td><td>{row.facts.length ? row.facts.join(" · ") : "Details hidden for privacy"}</td></tr>)}</tbody>
    </table></div>
    {!rows.length ? <p role="status">No preview rows match this filter.</p> : null}
    {filtered.length > pageSize ? <nav className="cleanup-record-preview__pagination" aria-label="Record preview pages"><span>{currentPage * pageSize + 1}–{currentPage * pageSize + rows.length} of {filtered.length} preview rows</span><div><button type="button" disabled={currentPage === 0} onClick={() => setPage(currentPage - 1)}>Previous</button><button type="button" disabled={(currentPage + 1) * pageSize >= filtered.length} onClick={() => setPage(currentPage + 1)}>Next</button></div></nav> : null}
  </section>;
}
