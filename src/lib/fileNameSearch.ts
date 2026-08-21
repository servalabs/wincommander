// SPDX-License-Identifier: AGPL-3.0-or-later
// Pure helpers for the filename (Everything-backed) half of Search Files:
// query building, result sorting, and display formatting. Ported verbatim
// from the panel component — behaviour is covered by the panel's
// long-standing semantics; keep changes surgical.

export interface SearchResult {
  name: string;
  directory: string;
  full_path: string;
  size: string;
  modified: string;
  icon_data?: string | null;
}

export interface SearchResponse {
  results: SearchResult[];
  total: number;
  query: string;
}

// Empty set = "All". Files / Folders are mutually exclusive (file vs folder
// scope); the rest (documents, images, videos, …) are extension filters and
// can be combined freely.
export type SearchType =
  | "files"
  | "folders"
  | "documents"
  | "images"
  | "videos"
  | "audio"
  | "archives"
  | "apps"
  | "code";
export type SizeFilter = "any" | "tiny" | "medium" | "large" | "huge";
export type DateFilter = "any" | "today" | "week" | "month";

// KT (FIX-D, measured on a real Everything index — see the task's evidence):
// every category below was missing mainstream, real-world extensions, and a
// missing extension is a silent "your file does not exist" — the worst
// failure mode a search box can have. Extensions were added conservatively —
// "what a normal person would call by that name" — with two deliberate
// exclusions worth flagging for a reviewer:
//   - "key" is NOT in documents despite Keynote using it: on Windows this
//     extension overwhelmingly means a TLS/SSH private key or a licence key,
//     and a "Documents" chip should not be a default way to surface key
//     material.
//   - "ts" is NOT in videos despite being a real MPEG transport-stream
//     extension: it already means TypeScript in the `code` set below, and
//     letting one extension mean two different chips would make Videos
//     surface source files and Code surface video streams.
export const FILE_TYPE_EXTENSIONS: Record<Exclude<SearchType, "files" | "folders">, string[]> = {
  // + odt (OpenDocument), epub (ebook), pages/numbers (iWork), djvu (scanned docs).
  documents: ["pdf", "doc", "docx", "txt", "md", "rtf", "ppt", "pptx", "xls", "xlsx", "csv", "odt", "epub", "pages", "numbers", "djvu"],
  // + avif, tif/tiff — measured 340 real files on this machine invisible to
  // the Images chip before this fix.
  images: ["jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "ico", "heic", "avif", "tif", "tiff"],
  // + mpg, mpeg, 3gp (mainstream video containers; not "ts", see note above).
  videos: ["mp4", "mkv", "mov", "avi", "wmv", "flv", "webm", "m4v", "mpg", "mpeg", "3gp"],
  // + m4b — an audiobook container, so it belongs here rather than in videos.
  audio: ["mp3", "wav", "flac", "m4a", "ogg", "aac", "wma", "m4b"],
  archives: ["zip", "rar", "7z", "tar", "gz", "iso", "cab"],
  // + apk — measured 51 real files on this machine invisible to the Apps chip.
  apps: ["exe", "msi", "appx", "appxbundle", "msix", "lnk", "apk"],
  // + c, h, hpp — measured ~35,120 real files invisible to the Code chip while
  // cpp/cs/java already shipped (an inconsistency, not a deliberate scope).
  // + sh, sql, toml, xml, vue, svelte, kt, swift, rb, php, lua — mainstream
  // source/config/markup formats in the same spirit as the pre-existing
  // html/css/json/yml (~14,000 more real files across these on this machine).
  code: ["js", "ts", "tsx", "jsx", "py", "rs", "go", "java", "cpp", "cs", "html", "css", "json", "yml", "yaml", "ps1", "bat", "cmd", "c", "h", "hpp", "sh", "sql", "toml", "xml", "vue", "svelte", "kt", "swift", "rb", "php", "lua"],
};

// Normalize separators to * and add trailing * for prefix matching
export function normalizeQuery(raw: string): string {
  const n = raw.trim().replace(/[\s\-,.]+/g, "*");
  return n.endsWith("*") ? n : `${n}*`;
}

// Known installed-app directory fragments (lowercased)
const APP_DIR_FRAGMENTS = [
  "program files",
  "program files (x86)",
  "\\appdata\\local\\programs",
  "\\appdata\\roaming\\microsoft\\windows\\start menu",
  "programdata\\microsoft\\windows\\start menu",
];

export function sfExtOf(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot >= 0 ? name.slice(dot + 1).toLowerCase() : "";
}

export function isDirectoryResult(result: SearchResult): boolean {
  return result.size === "0" || result.size === "";
}

export function sfAppSortScore(result: SearchResult): number {
  const ext = sfExtOf(result.name);
  if (ext === "lnk") return 0;
  if (ext === "exe" || ext === "msi" || ext === "appx" || ext === "msix") {
    const dir = result.directory.toLowerCase();
    if (APP_DIR_FRAGMENTS.some(f => dir.includes(f))) return 1;
    return 2;
  }
  if (isDirectoryResult(result)) return 4; // folders last
  return 3;
}

// KT: this used to be two functions — buildSearchQuery (full tokens) and a
// buildBackendSearchQuery wrapper that sent ONLY the free-text query to the
// backend when filters were also active, then re-filtered the raw hits
// client-side via a since-deleted resultMatchesFilters(). The workaround
// existed because es.exe (Everything's CLI) joins its non-flag argv entries
// with spaces, but a single argv entry that CONTAINS a space is treated as a
// quoted phrase — so passing "folder: assets*" as one shell argument
// silently returned zero rows. That made a confident empty result the worst
// failure mode: "folders named assets" could report "no matches" while
// thousands of matching folders sat in the index, because the first N raw
// hits (fetched from `assets*` alone) happened to all be files.
//
// The backend now tokenizes each `ext:`/`size:`/`dm:`/etc. term into its own
// argv entry (see search_everything / es_query.rs), so it can honour every
// filter server-side. This is now the single, always-correct query builder —
// there is no longer a case where sending fewer tokens is necessary or
// safer, so the separate buildBackendSearchQuery wrapper and the
// client-side resultMatchesFilters() re-filter are both deleted rather than
// kept as redundant (and, per the date-window mismatch below, actively
// wrong) safety nets. Everything's `dm:thisweek` means "since the start of
// this calendar week"; the deleted client filter used "last 7 rolling days
// from local midnight" — those disagree, so running both would have
// silently dropped backend-correct rows.
export function buildSearchQuery(q: string, types: Set<SearchType>, size: SizeFilter, date: DateFilter): string {
  const tokens: string[] = [];
  const trimmed = q.trim();
  if (types.has("files")) tokens.push("file:");
  if (types.has("folders")) tokens.push("folder:");
  // Combine extensions from every selected category — multi-select friendly.
  const extCats = Array.from(types).filter((t) => t !== "files" && t !== "folders");
  if (extCats.length > 0) {
    const seen = new Set<string>();
    for (const t of extCats) {
      for (const ext of FILE_TYPE_EXTENSIONS[t as keyof typeof FILE_TYPE_EXTENSIONS]) {
        seen.add(ext);
      }
    }
    tokens.push(`ext:${Array.from(seen).join(";")}`);
  }
  if (size === "tiny") tokens.push("size:<1mb");
  if (size === "medium") tokens.push("size:1mb..100mb");
  if (size === "large") tokens.push("size:>100mb");
  if (size === "huge") tokens.push("size:>1gb");
  if (date === "today") tokens.push("dm:today");
  if (date === "week") tokens.push("dm:thisweek");
  if (date === "month") tokens.push("dm:thismonth");
  if (trimmed) tokens.push(normalizeQuery(trimmed));
  return tokens.filter(Boolean).join(" ");
}

export function formatResultSize(size: string): string {
  const n = parseInt(size, 10);
  if (isNaN(n)) return size;
  if (n === 0) return "—";
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

/** True when the filename engine (Everything/es.exe) is absent or stopped —
 *  the panel degrades to an inline note instead of a screaming error box. */
export function isEngineMissingError(error: string | null): boolean {
  if (!error) return false;
  const lower = error.toLowerCase();
  return lower.includes("search engine not installed")
    || lower.includes("search engine service is not running")
    || lower.includes("not found")
    || lower.includes("ipc not found");
}

/** A timed-out autocomplete request is transient load, not a user-facing
 * search failure. The shortcut retries on the next settled input state. */
export function isSearchTimeoutError(error: unknown): boolean {
  return String(error).toLowerCase().includes("search took too long");
}
