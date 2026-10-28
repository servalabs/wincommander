import type { CleanupDisplayMode } from "../../panels/cleanup/cleanupDisplay";

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
  for (const key of ["records", "entries", "files", "logs", "devices", "journals", "copies", "dumps", "databases", "processes"]) {
    const values = input[key];
    if (!Array.isArray(values) || !values.length) continue;
    return values.slice(0, 20).map((value, index) => ({ label: `Record ${index + 1}`, facts: isObject(value) ? metadata(value) : [] }));
  }
  return [];
}

export default function CleanupRecordPreview({ categoryId, rawData, mode }: { categoryId: string; rawData: unknown; mode: CleanupDisplayMode }) {
  const all = getSafeCleanupPreview(categoryId, rawData);
  if (!all.length) return null;
  const rows = all.slice(0, mode === "simple" ? 5 : 20);
  return <section className="cleanup-record-preview" aria-label="Record preview">
    <h3>Record preview</h3>
    <p>Showing the first {rows.length} {rows.length === 1 ? "preview row" : "preview rows"} supplied by this scan. The total is shown above. Paths, credentials and private contents are hidden.</p>
    <ul>{rows.map((row, index) => <li key={`${row.label}:${index}`}><strong>{row.label}</strong>{row.facts.length ? <span>{row.facts.join(" · ")}</span> : <span>Additional details are hidden for privacy.</span>}</li>)}</ul>
  </section>;
}
