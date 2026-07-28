// SPDX-License-Identifier: AGPL-3.0-or-later
// src/hooks/useChipSearch.ts
//
// State + IPC behind the Ctrl+Space chip search overlay. EverythingSearchBar
// stays a pure renderer over this hook: everything that debounces, invokes,
// ranks, or remembers lives here, and the pure query grammar lives one layer
// further down in src/lib/searchTokens.ts + src/lib/searchQueryPlan.ts.
//
// Three behaviours are worth knowing before editing:
//   1. The plan's `tokens` array goes to the backend as `tokens`, NEVER joined
//      into `query`. es.exe treats one argv entry containing a space as a quoted
//      phrase, so a joined multi-token query returns zero rows with no error.
//   2. An empty box is a BROWSE, not an idle state — the most common reason to
//      open a launcher is "the thing I was just working on", so we over-fetch a
//      recency-sorted page, drop the machine noise, and let frecency lead.
//   3. Everything the backend might not have (the count command, the Explorer
//      probe) degrades to "no extra information", never to an error the user
//      cannot act on.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { buildContentQueryArgs, contentHitToDisplayRow } from "@/lib/contentSearch";
import type { ContentDisplayRow } from "@/lib/contentSearch";
import type { ContentHit } from "@/types/wincmd-search";
import { sfExtOf } from "@/lib/fileNameSearch";
import type { SearchResponse, SearchResult } from "@/lib/fileNameSearch";
import { resolveMotionDisabled } from "@/lib/motionPolicy";
import { topPaths } from "@/lib/frecency";
import { buildContentTerms, buildEverythingPlan, contentSearchApplies } from "@/lib/searchQueryPlan";
import type { EverythingPlan } from "@/lib/searchQueryPlan";
import { EMPTY_QUERY, addChip, chipOf, parseFolderJump, suggestChip } from "@/lib/searchTokens";
import type { ChipSuggestion, QueryState } from "@/lib/searchTokens";

/** Rows that carry no real stat data — reconstructed from a remembered path. */
export type BrowseResult = SearchResult & { synthetic?: boolean };

export interface ExplorerFolder { path: string; label: string }

const NAME_DEBOUNCE_MS = 200;
const CONTENT_DEBOUNCE_MS = 275;
/** Over-fetch the browse page so noise filtering still leaves a full list. */
const BROWSE_FETCH = 60;
const BROWSE_SHOW = 10;
/** KT: over-fetch generously — we only ever SHOW MATCH_SHOW rows, but every
 *  ranking signal we apply (app priority, frecency, exact-name, path noise) can
 *  only reorder what the engine already handed us. A narrow window is how you
 *  get a confident empty answer: a common term like "assets" has ~1200 hits, so
 *  a 50-row window plus client-side sorting is just "sort the arbitrary 50 you
 *  happened to receive". Fetching 300 rows measures at 0.096s against es.exe,
 *  so the window is essentially free and the ranking finally has material. */
const MATCH_FETCH = 300;
const MATCH_SHOW = 10;
/** How many remembered paths may lead the browse list. */
const FRECENCY_LEAD = 4;
const APP_EXT_TOKEN = "ext:exe;lnk;msi;appx;msix";

// Paths that are technically "recently modified" and never what anyone meant.
// Matched per path segment (both separators padded on) so a user folder called
// "Temp Renders" survives while %TEMP% itself does not.
const NOISE_SEGMENTS = [
  "\\appdata\\", "\\temp\\", "\\node_modules\\",
  "\\.git\\", "\\$recycle.bin\\", "\\winsxs\\",
];

export function isNoisePath(path: string): boolean {
  const padded = `${path.toLowerCase().replace(/\//g, "\\")}\\`;
  return NOISE_SEGMENTS.some((seg) => padded.includes(seg));
}

// KT: the missing ranking signal. A generic term like "assets" matches ~1200
// folders on a real machine, and once ties are broken only by name length they
// are ALL exactly "assets" — so whatever the engine happened to return first won,
// which was Office update trees and `…\.git\modules\assets`, never the
// developer's own `D:\GitHub\wincommander\assets`. People look for their own
// documents and projects far more often than for a file inside an installed
// program, so path location is itself a relevance signal.
//
// Deliberately NOT keyed on "\appdata\": plenty of real apps install under
// %LOCALAPPDATA%\Programs (VS Code, Discord, GitHub Desktop). And this only ever
// reorders rows WITHIN an appSortScore tier, so brave.exe (tier 0) keeps its
// place regardless of living in C:\Program Files.
const MACHINE_SEGMENTS = [
  "\\node_modules\\", "\\.git\\", "\\$recycle.bin\\",
  "\\winsxs\\", "\\temp\\", "\\installer\\",
  // Any segment ending in "cache" — catches \.cache\, \Package Cache\ and
  // \marketplace-cache\ without enumerating each one.
  //
  // KT: "\\dist\\" was tried here and REMOVED. It demoted real project folders
  // (a checked-in `…/investigator-app/dist/assets`) below crate-registry noise,
  // so it made the visible answer worse. Build output is still something people
  // legitimately search for. The lesson: past the obvious machine trees, path
  // heuristics start trading one noise class for another — the signal that
  // actually resolves a tie between two folders in the user's own space is
  // frecency (what they open), not a longer denylist.
  "cache\\",
];
const SYSTEM_SEGMENTS = [
  "\\windows\\", "\\program files\\", "\\program files (x86)\\",
  "\\programdata\\", "\\windowsapps\\",
];

/**
 * Location preference, lower = more likely to be what the user meant.
 *   0  the user's own space (Documents, Desktop, project drives)
 *   1  an installed program's tree
 *   2  a machine-generated tree (build output, VCS internals, caches)
 * A demotion, never a filter: a file that only exists at level 2 is still
 * findable, it just never outranks the user's own copy.
 */
export function pathPreference(path: string): number {
  const padded = `${path.toLowerCase().replace(/\//g, "\\")}\\`;
  if (MACHINE_SEGMENTS.some((seg) => padded.includes(seg))) return 2;
  if (SYSTEM_SEGMENTS.some((seg) => padded.includes(seg))) return 1;
  return 0;
}

// KT: mirrors frecency.ts's module-private normalizeKey. Windows paths are
// case-insensitive and accept either separator, so "D:/x/Y.txt" and
// "D:\X\y.TXT" must hash to the same history entry or the ranking silently
// never matches anything the user actually opened.
function frecKey(path: string): string {
  return path.trim().replace(/\\/g, "/").toLowerCase();
}

const APP_EXTS = new Set(["exe", "msi", "appx", "msix", "lnk"]);

function nameWithoutExt(name: string): string {
  const dot = name.lastIndexOf(".");
  return (dot >= 0 ? name.slice(0, dot) : name).toLowerCase();
}

// Sort priority — lower shows first. Apps ALWAYS beat data files regardless of
// where they live, because path-based scoring missed apps installed under
// %LOCALAPPDATA%\GitHubDesktop, \Discord, and friends.
//   0 exact basename on an app extension · 1 prefix match on one ·
//   2 any .lnk · 3 any other app binary · 4 everything else
function appSortScore(result: SearchResult, termLower: string): number {
  const ext = sfExtOf(result.name);
  if (APP_EXTS.has(ext)) {
    const base = nameWithoutExt(result.name);
    if (base === termLower) return 0;
    if (termLower && base.startsWith(termLower)) return 1;
  }
  if (ext === "lnk") return 2;
  if (ext === "exe" || ext === "msi" || ext === "appx" || ext === "msix") return 3;
  return 4;
}

/** Rank lookup built from ONE localStorage read. Calling frecencyScore per
 *  comparison would re-parse the whole history for every sort step. */
function frecencyRanks(): Map<string, number> {
  const ranks = new Map<string, number>();
  topPaths(500).forEach((path, index) => ranks.set(frecKey(path), index));
  return ranks;
}

/** True while a second, app-only query can safely run alongside the general
 *  one. A plan that already carries `ext:` / `folder:` / `empty:` / `dupe:`
 *  cannot: a second `ext:` token ANDs with the first and returns zero rows. */
function appsBoostApplies(plan: EverythingPlan): boolean {
  if (plan.isBrowse) return false;
  return !plan.tokens.some(
    (t) => t.startsWith("ext:") || t === "folder:" || t === "empty:" || t === "dupe:",
  );
}

// Windows built-ins that are not reliably indexed as regular files: UWP apps
// live in WindowsApps (skipped by default) and shell-only entry points like
// Settings have no file at all — they are URI-launchable. When a query matches
// an entry's keywords we inject it at the top so these always show up.
interface BuiltinApp { name: string; keywords: string[]; path: string }
const BUILTIN_APPS: readonly BuiltinApp[] = [
  { name: "File Explorer", keywords: ["file explorer", "explorer", "files"], path: "C:\\Windows\\explorer.exe" },
  { name: "Settings", keywords: ["settings", "windows settings"], path: "ms-settings:" },
  { name: "Control Panel", keywords: ["control panel", "control"], path: "C:\\Windows\\System32\\control.exe" },
  { name: "Task Manager", keywords: ["task manager", "taskmgr"], path: "C:\\Windows\\System32\\Taskmgr.exe" },
  { name: "Command Prompt", keywords: ["cmd", "command prompt"], path: "C:\\Windows\\System32\\cmd.exe" },
  { name: "PowerShell", keywords: ["powershell", "pwsh", "ps"], path: "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe" },
  { name: "Calculator", keywords: ["calc", "calculator"], path: "calculator:" },
  { name: "Notepad", keywords: ["notepad"], path: "C:\\Windows\\System32\\notepad.exe" },
  { name: "Paint", keywords: ["paint", "mspaint"], path: "C:\\Windows\\System32\\mspaint.exe" },
  { name: "Snipping Tool", keywords: ["snip", "snipping tool", "screenshot"], path: "C:\\Windows\\System32\\SnippingTool.exe" },
  { name: "Registry Editor", keywords: ["regedit", "registry"], path: "C:\\Windows\\regedit.exe" },
  { name: "Run", keywords: ["run"], path: "C:\\Windows\\System32\\rundll32.exe" },
  { name: "Device Manager", keywords: ["device manager", "devmgmt"], path: "C:\\Windows\\System32\\devmgmt.msc" },
  { name: "Disk Management", keywords: ["disk management", "diskmgmt"], path: "C:\\Windows\\System32\\diskmgmt.msc" },
  { name: "Services", keywords: ["services", "services.msc"], path: "C:\\Windows\\System32\\services.msc" },
  { name: "System Configuration", keywords: ["msconfig", "system configuration"], path: "C:\\Windows\\System32\\msconfig.exe" },
];

function builtinMatches(termLower: string): BrowseResult[] {
  if (!termLower) return [];
  return BUILTIN_APPS
    .filter((app) => app.keywords.some((k) => k.startsWith(termLower) || termLower.startsWith(k)))
    .map((app) => ({
      // The ".exe" suffix is what drives the app icon and the top sort tier.
      // `size: "1"` (not "") matters: "" reads as "directory" to the row
      // renderer, which would skip the shell icon fetch and paint a folder on
      // Notepad. `synthetic` then suppresses the meaningless size cell.
      name: `${app.name}.exe`,
      directory: app.path.endsWith(":") ? "Windows shell" : app.path.slice(0, app.path.lastIndexOf("\\")),
      full_path: app.path,
      size: "1",
      modified: "",
      icon_data: null,
      synthetic: true,
    }));
}

function fetchNames(plan: EverythingPlan, limit: number, extra?: string): Promise<SearchResponse> {
  const tokens = extra ? [...plan.tokens, extra] : plan.tokens;
  return invoke<SearchResponse>("search_everything", {
    // `tokens` wins over `query` in the backend; the empty string keeps the
    // required parameter satisfied without ever being parsed.
    query: "",
    maxResults: limit,
    tokens,
    sort: plan.sort,
    scopePath: plan.scopePath,
  });
}

// KT: frecency stores paths and nothing else, so a remembered file that fell
// outside Everything's recency window has to be rebuilt from its path. `size`
// is how the row renderer spells "directory" ("" or "0"), and `synthetic`
// suppresses the size cell — guessing wrong would print "1 B" on every row.
function synthesizeRow(path: string): BrowseResult {
  const norm = path.replace(/\//g, "\\");
  const cut = norm.lastIndexOf("\\");
  const name = cut >= 0 ? norm.slice(cut + 1) : norm;
  return {
    name,
    directory: cut > 0 ? norm.slice(0, cut) : "",
    full_path: path,
    size: name.includes(".") ? "1" : "0",
    modified: "",
    synthetic: true,
  };
}

/** The empty-box list: recency from the index, led by what the user opens. */
async function fetchBrowse(plan: EverythingPlan, filtered: boolean): Promise<BrowseResult[]> {
  const resp = await fetchNames(plan, BROWSE_FETCH);
  const rows: BrowseResult[] = resp.results.filter((r) => !isNoisePath(r.full_path));
  // With chips active the list is a filtered search, not a history — injecting
  // remembered paths there would show rows that contradict the visible filters.
  if (filtered) return rows.slice(0, BROWSE_SHOW);

  const lead: BrowseResult[] = [];
  const leadKeys = new Set<string>();
  for (const path of topPaths(FRECENCY_LEAD * 4)) {
    if (lead.length >= FRECENCY_LEAD) break;
    if (isNoisePath(path)) continue;
    const key = frecKey(path);
    if (leadKeys.has(key)) continue;
    leadKeys.add(key);
    lead.push(rows.find((r) => frecKey(r.full_path) === key) ?? synthesizeRow(path));
  }
  const rest = rows.filter((r) => !leadKeys.has(frecKey(r.full_path)));
  return [...lead, ...rest].slice(0, BROWSE_SHOW);
}

/** The typed-query list: apps first, then frecency, then shortest name. */
async function fetchMatches(plan: EverythingPlan, term: string): Promise<BrowseResult[]> {
  const termLower = term.toLowerCase();
  const boost = appsBoostApplies(plan);
  const [apps, all] = await Promise.all([
    boost
      ? fetchNames(plan, 30, APP_EXT_TOKEN).catch(() => ({ results: [], total: 0, query: "" }))
      : Promise.resolve<SearchResponse>({ results: [], total: 0, query: "" }),
    fetchNames(plan, MATCH_FETCH),
  ]);

  const ranks = frecencyRanks();
  const byScore = (a: SearchResult, b: SearchResult) => {
    const tier = appSortScore(a, termLower) - appSortScore(b, termLower);
    if (tier !== 0) return tier;
    // Frecency breaks ties WITHIN a tier rather than across them: what you open
    // daily should win among equals, but never push brave.exe below a .txt.
    const ra = ranks.get(frecKey(a.full_path)) ?? Number.MAX_SAFE_INTEGER;
    const rb = ranks.get(frecKey(b.full_path)) ?? Number.MAX_SAFE_INTEGER;
    if (ra !== rb) return ra - rb;
    // The user's own space outranks program trees, which outrank build/cache
    // output. This sits AFTER frecency on purpose: if you actually open something
    // inside node_modules, it is not noise to you and keeps the rank you earned.
    const where = pathPreference(a.full_path) - pathPreference(b.full_path);
    if (where !== 0) return where;
    return a.name.length - b.name.length;
  };

  // Built-ins lead when their keywords match, but only inside the same gate as
  // the apps boost: an [Images] chip or an `in Downloads` scope must not surface
  // C:\Windows\explorer.exe, which satisfies neither.
  const leading = boost && !plan.scopePath ? builtinMatches(termLower) : [];
  const seen = new Set<string>();
  const merged: BrowseResult[] = [];
  for (const row of [
    ...leading,
    ...[...apps.results].sort(byScore),
    ...[...all.results].sort(byScore),
  ]) {
    if (seen.has(row.full_path)) continue;
    seen.add(row.full_path);
    merged.push(row);
  }
  return merged.slice(0, MATCH_SHOW);
}

export interface ChipSearchApi {
  query: QueryState;
  setQuery: React.Dispatch<React.SetStateAction<QueryState>>;
  /** Trailing-word chip candidate. Provisional until Tab. */
  suggestion: ChipSuggestion | null;
  /** Explorer's last-viewed folder, while it is still worth offering. */
  explorerOffer: ExplorerFolder | null;
  acceptExplorerOffer: () => void;
  dismissExplorerOffer: () => void;
  /** True when the rows CURRENTLY on screen are a recency browse, not a search. */
  isBrowse: boolean;
  /** True for a ">folder" jump query. */
  isJump: boolean;
  /** The search term with any ">" prefix stripped. */
  term: string;
  primary: BrowseResult[];
  contentRows: ContentDisplayRow[];
  /** True total from the engine, or null when unknown / not worth showing. */
  totalCount: number | null;
  isSearching: boolean;
  error: string | null;
  setError: (message: string | null) => void;
  /** Clear everything and re-probe the Explorer folder. Call on every open. */
  reset: () => void;
}

export function useChipSearch(active: boolean): ChipSearchApi {
  const [query, setQuery] = useState<QueryState>(EMPTY_QUERY);
  const [primary, setPrimary] = useState<BrowseResult[]>([]);
  // Tracks what the CURRENT rows are, not what the pending plan wants. Reading
  // plan.isBrowse in the UI made the "Recent" header vanish the instant a key
  // went down and the rows follow 200ms later, so the list sat there unlabelled.
  const [showingBrowse, setShowingBrowse] = useState(true);
  const [contentRows, setContentRows] = useState<ContentDisplayRow[]>([]);
  const [totalCount, setTotalCount] = useState<number | null>(null);
  const [isSearching, setIsSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [explorerFolder, setExplorerFolder] = useState<ExplorerFolder | null>(null);
  const [offerUsed, setOfferUsed] = useState(false);

  const runIdRef = useRef(0);
  const nameTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const contentTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const jump = useMemo(() => parseFolderJump(query.text), [query.text]);

  // ">assets" means "take me into that folder", so the search is folders-only —
  // but the chip row is left alone. A Folders chip the user never added would
  // outlive the ">" they can delete, which reads as the box filtering itself.
  const searchState = useMemo<QueryState>(() => {
    if (!jump.isJump) return query;
    if (chipOf(query, "folders") || chipOf(query, "files")) return query;
    return addChip(query, "folders");
  }, [jump.isJump, query]);

  const plan = useMemo(() => buildEverythingPlan(searchState), [searchState]);
  const suggestion = useMemo(() => suggestChip(query), [query]);

  const probeExplorerFolder = useCallback(() => {
    setOfferUsed(false);
    invoke<ExplorerFolder | null>("get_foreground_explorer_folder")
      .then((folder) => setExplorerFolder(folder ?? null))
      // No Explorer window, COM refused, or the command not registered yet —
      // all the same nothing-to-offer case, never an error.
      .catch(() => setExplorerFolder(null));
  }, []);

  const reset = useCallback(() => {
    runIdRef.current += 1; // orphan any in-flight response
    setQuery(EMPTY_QUERY);
    setPrimary([]);
    setShowingBrowse(true);
    setContentRows([]);
    setTotalCount(null);
    setError(null);
    setIsSearching(false);
    probeExplorerFolder();
  }, [probeExplorerFolder]);

  useEffect(() => {
    if (active) probeExplorerFolder();
  }, [active, probeExplorerFolder]);

  // ── Filename / browse pass ──
  useEffect(() => {
    if (nameTimerRef.current) clearTimeout(nameTimerRef.current);
    if (!active) return;
    const runId = ++runIdRef.current;
    const live = () => runIdRef.current === runId;

    nameTimerRef.current = setTimeout(() => {
      setIsSearching(true);
      void (async () => {
        try {
          const rows = plan.isBrowse
            ? await fetchBrowse(plan, searchState.chips.length > 0)
            : await fetchMatches(plan, jump.term);
          if (!live()) return;
          setPrimary(rows);
          setShowingBrowse(plan.isBrowse);
          setError(null);
        } catch (err) {
          if (!live()) return;
          setPrimary([]);
          setShowingBrowse(plan.isBrowse);
          setError(String(err));
        } finally {
          if (live()) setIsSearching(false);
        }
        // The count is a second, slower round-trip that must never gate the
        // rows. With no tokens at all it would report "every file on this PC",
        // which is noise rather than information.
        if (!live() || plan.tokens.length === 0) {
          if (live()) setTotalCount(null);
          return;
        }
        try {
          const total = await invoke<number>("search_everything_count", {
            query: "",
            tokens: plan.tokens,
            scopePath: plan.scopePath,
          });
          if (live()) setTotalCount(total);
        } catch {
          if (live()) setTotalCount(null);
        }
      })();
    }, NAME_DEBOUNCE_MS);

    return () => {
      if (nameTimerRef.current) clearTimeout(nameTimerRef.current);
    };
  }, [active, plan, searchState, jump.term]);

  // ── Content pass (best-effort, silent on failure) ──
  useEffect(() => {
    if (contentTimerRef.current) clearTimeout(contentTimerRef.current);
    if (!active) return;
    if (jump.term.length < 2 || !contentSearchApplies(searchState)) {
      setContentRows([]);
      return;
    }
    let cancelled = false;
    const terms = buildContentTerms(searchState);
    contentTimerRef.current = setTimeout(() => {
      invoke<ContentHit[]>(
        "search_content",
        buildContentQueryArgs(terms, 5) as unknown as Record<string, unknown>,
      )
        .then((hits) => { if (!cancelled) setContentRows(hits.map(contentHitToDisplayRow)); })
        .catch(() => { /* the bar must not break when the index is cold */ });
    }, CONTENT_DEBOUNCE_MS);

    return () => {
      cancelled = true;
      if (contentTimerRef.current) clearTimeout(contentTimerRef.current);
    };
  }, [active, jump.term, searchState]);

  const explorerOffer = useMemo(() => {
    if (offerUsed || !explorerFolder) return null;
    return chipOf(query, "in") ? null : explorerFolder;
  }, [offerUsed, explorerFolder, query]);

  const acceptExplorerOffer = useCallback(() => {
    const folder = explorerFolder;
    if (!folder) return;
    setOfferUsed(true);
    setQuery((prev) => addChip(prev, "in", { path: folder.path, pathLabel: folder.label }));
  }, [explorerFolder]);

  const dismissExplorerOffer = useCallback(() => setOfferUsed(true), []);

  return {
    query, setQuery, suggestion,
    explorerOffer, acceptExplorerOffer, dismissExplorerOffer,
    isBrowse: showingBrowse, isJump: jump.isJump, term: jump.term,
    primary, contentRows, totalCount, isSearching, error, setError, reset,
  };
}

/**
 * Reduced-motion resolver for the overlay window.
 *
 * KT: useMotionPreference() cannot be used here — the "search-overlay" webview
 * (see main.tsx) mounts this component with only ThemeProvider, so its
 * useAuthMode()/useAppState() reads would throw. resolveMotionDisabled() is the
 * same policy without the context dependency.
 */
export function useReducedMotionPref(): boolean {
  const [reduced, setReduced] = useState<boolean>(() => resolveMotionDisabled());

  useEffect(() => {
    if (typeof window === "undefined" || typeof document === "undefined") return;
    const recompute = () => setReduced(resolveMotionDisabled());
    const mq = window.matchMedia?.("(prefers-reduced-motion: reduce)");
    mq?.addEventListener?.("change", recompute);
    const observer =
      typeof MutationObserver === "undefined" ? undefined : new MutationObserver(recompute);
    observer?.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });
    return () => {
      mq?.removeEventListener?.("change", recompute);
      observer?.disconnect();
    };
  }, []);

  return reduced;
}
