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
import { isDirectoryResult, isSearchTimeoutError, sfExtOf } from "@/lib/fileNameSearch";
import type { SearchResponse, SearchResult } from "@/lib/fileNameSearch";
import { resolveMotionDisabled } from "@/lib/motionPolicy";
import { normalizeKey, topPaths } from "@/lib/frecency";
import { buildContentTerms, buildEverythingPlan, contentSearchApplies, splitScopePaths } from "@/lib/searchQueryPlan";
import type { EverythingPlan } from "@/lib/searchQueryPlan";
import { EMPTY_QUERY, addChip, chipOf, parseFolderJump, suggestChip } from "@/lib/searchTokens";
import type { ChipSuggestion, QueryState } from "@/lib/searchTokens";

/** Rows that carry no real stat data — reconstructed from a remembered path. */
export type BrowseResult = SearchResult & { synthetic?: boolean };

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

// KT: MEASURED — 1-2 character terms run against the WHOLE index timed out 9
// of 16 runs (56%) under load, versus 0 of 8 for a 20-char term. A term this
// short is virtually always still mid-keystroke, so refusing it client-side
// before it ever reaches es.exe costs nothing real and protects the single
// shared IPC instance from its most timeout-prone request shape. Deliberately
// NOT applied to the browse pass: an empty term means isBrowse, a different
// code path entirely, so `isMatchTermTooShort("")` must stay false or a
// chip-only query (e.g. just the "Images" chip, no typed text) would wrongly
// get refused too.
export const MIN_MATCH_TERM_LENGTH = 3;
export function isMatchTermTooShort(term: string): boolean {
  return term.length > 0 && term.length < MIN_MATCH_TERM_LENGTH;
}
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

// KT: this was a hand-copied duplicate of frecency.ts's normaliser, and the copy
// went stale exactly as you would expect — the original grew handling for
// trailing separators, doubled separators and `\\?\` extended-length prefixes
// while this one did not, so the two disagreed about whether two spellings of
// one real path were the same file. A disagreement here is silent: the history
// lookup simply never matches and frecency contributes nothing to ranking.
// Import the single source of truth instead of mirroring it.
const frecKey = normalizeKey;

const APP_EXTS = new Set(["exe", "msi", "appx", "msix", "lnk"]);

// KT: MEASURED — this corpus's real "folders" stratum is dominated by hidden/
// config directories (.claude-plugin, .cargo, .bin, .claude, .git, …), every
// dev machine's most common folder-name shape. A dot at position 0 is a
// hidden-file marker, not an extension delimiter: `dot >= 0` treated
// ".claude-plugin" as ext="claude-plugin" with an EMPTY basename, so it could
// never register as an exact/prefix match against anything — the new tiers
// above silently never engaged for the exact folder the sharpest measured
// case (`…\ralph-loop\.claude-plugin`) is shaped like. `dot > 0` leaves a
// REAL extension (".env.local" -> base ".env") untouched; only a name whose
// ONLY dot is the leading one now keeps its whole name as the basename.
function nameWithoutExt(name: string): string {
  const dot = name.lastIndexOf(".");
  return (dot > 0 ? name.slice(0, dot) : name).toLowerCase();
}

// Sort priority — lower shows first.
//   0 exact basename match, on an app extension    (brave.exe for "brave")
//   1 exact basename match, on ANY type             (a folder/doc named EXACTLY the term)
//   2 prefix basename match, on an app extension    (BraveUpdate.exe for "brave")
//   3 prefix basename match, on ANY type             (a folder/doc that STARTS WITH the term)
//   4 any .lnk, no name match at all
//   5 any other app binary, no name match at all
//   6 a directory, no name match at all
//   7 everything else
//
// KT: MEASURED (2073 real queries): tier 0/1 used to exist ONLY for app
// extensions, so a folder or a .txt file named EXACTLY what the user typed
// shared tier 4 ("everything else") with 300 unrelated rows, tied only on
// pathPreference() then shorter-name-wins — an exact match had no advantage
// over noise. Biggest loss point in the product: recall@10 was 44.8% vs
// recall@300's 70.6%, and 535 of the misses (46.8%) were rows the engine had
// ALREADY FETCHED — the comparator just never surfaced them. Folders paid
// worst: recall@10 8.6% vs recall@300 66.9%, because this function never
// looked at whether a result was a directory at all.
//
// JUDGEMENT CALL — exact match of ANY type (tier 1) outranks an app that only
// PREFIX-matches (tier 2): an exact hit is a stronger intent signal than
// "happens to start with the same letters and is also runnable" — a folder
// named exactly "chrome" is more likely the target than `ChromeSetup.exe`.
// The app-priority invariant survives exactly where it was measured to
// matter — true ties and true non-matches: an app that IS ITSELF an exact
// match (tier 0) still beats a non-app exact match (tier 1), so "brave" with
// both `Brave.lnk` and `brave.svg` on disk still offers the browser first.
// And an app with ZERO name relation (tiers 4/5) still beats a non-matching
// file OR folder (tiers 6/7) — unchanged, since that rule recovers apps
// installed under %LOCALAPPDATA% that path scoring alone couldn't reach.
//
// Tier 6 (directory, no match) is new, below the unconditional app tiers but
// above plain files: sfAppSortScore in fileNameSearch.ts ranks directories
// LAST, correct for that file-oriented panel but wrong for a launcher where
// "take me to my project folder" is the headline intent — a folder that
// isn't even a name match still shouldn't drown behind every unrelated file.
export function appSortScore(result: SearchResult, termLower: string): number {
  const ext = sfExtOf(result.name);
  const isApp = APP_EXTS.has(ext);
  const base = nameWithoutExt(result.name);
  const isExact = base === termLower;
  const isPrefix = !isExact && termLower !== "" && base.startsWith(termLower);

  if (isApp && isExact) return 0;
  if (isExact) return 1;
  if (isApp && isPrefix) return 2;
  if (isPrefix) return 3;
  if (ext === "lnk") return 4;
  if (ext === "exe" || ext === "msi" || ext === "appx" || ext === "msix") return 5;
  if (isDirectoryResult(result)) return 6;
  return 7;
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

const EMPTY_SEARCH_RESPONSE: SearchResponse = { results: [], total: 0, query: "" };

/** The typed-query list: exact/prefix name matches first (apps ahead of ties),
 *  then frecency, then shortest name. */
async function fetchMatches(plan: EverythingPlan, term: string): Promise<BrowseResult[]> {
  const termLower = term.toLowerCase();
  const boost = appsBoostApplies(plan);
  // The overlay previously launched three es.exe requests on every settled
  // keystroke. Everything serialises enough work internally that those calls
  // queued behind each other and the 6 s safety timeout surfaced as a bogus
  // "filter isn't indexed" error. The recency page is the useful answer in
  // almost every case; only ask for the expensive recovery pages when it is
  // genuinely sparse. A timeout is then a silent partial result, never an
  // alarming user-facing error for a query that remains valid.
  const sorted = await fetchNames(plan, MATCH_FETCH).catch((error) => {
    if (isSearchTimeoutError(error)) return EMPTY_SEARCH_RESPONSE;
    throw error;
  });
  const needsRecovery = sorted.results.length < MATCH_SHOW;
  const [apps, unsorted] = needsRecovery
    ? await Promise.all([
      boost ? fetchNames(plan, 30, APP_EXT_TOKEN).catch(() => EMPTY_SEARCH_RESPONSE) : Promise.resolve<SearchResponse>(EMPTY_SEARCH_RESPONSE),
      fetchNames({ ...plan, sort: undefined }, MATCH_FETCH).catch(() => EMPTY_SEARCH_RESPONSE),
    ])
    : [EMPTY_SEARCH_RESPONSE, EMPTY_SEARCH_RESPONSE];
  // ONE pool, sorted ONCE below — not two independently-sorted arrays
  // concatenated, which would strand an unsorted-only exact match behind up to
  // 300 sorted-page rows regardless of its own tier.
  const all = [...sorted.results, ...unsorted.results];

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
    ...all.sort(byScore),
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

/** Return a user-facing reason to suppress a query before it reaches either index. */
export type ChipSearchBlocker = (state: QueryState) => string | null;

export function useChipSearch(active: boolean, blockSearch?: ChipSearchBlocker): ChipSearchApi {
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
  const blockedReason = useMemo(
    () => blockSearch?.(searchState) ?? null,
    [blockSearch, searchState],
  );

  const reset = useCallback(() => {
    runIdRef.current += 1; // orphan any in-flight response
    setQuery(EMPTY_QUERY);
    setPrimary([]);
    setShowingBrowse(true);
    setContentRows([]);
    setTotalCount(null);
    setError(null);
    setIsSearching(false);
  }, []);

  // A corrected drive request is a new query, not a failed retry. Remove the
  // previous scope message immediately while the normal debounced search runs.
  useEffect(() => {
    if (!blockedReason) setError(null);
  }, [blockedReason]);

  // ── Filename / browse pass ──
  useEffect(() => {
    if (nameTimerRef.current) clearTimeout(nameTimerRef.current);
    if (!active) return;
    const runId = ++runIdRef.current;
    const live = () => runIdRef.current === runId;

    // A missing drive is not a query with zero results. Suppress it before
    // either engine sees its words, or unrelated matches make the storage
    // request look as though it was ignored.
    if (blockedReason) {
      setPrimary([]);
      setShowingBrowse(false);
      setContentRows([]);
      setTotalCount(null);
      setError(blockedReason);
      setIsSearching(false);
      return;
    }

    nameTimerRef.current = setTimeout(() => {
      // A too-short term is refused before it ever reaches es.exe — see
      // isMatchTermTooShort. Never true for a browse (empty term), so this
      // cannot swallow the empty-box or chip-only cases.
      if (!plan.isBrowse && isMatchTermTooShort(jump.term)) {
        setPrimary([]);
        setShowingBrowse(false);
        setError(null);
        setTotalCount(null);
        setIsSearching(false);
        return;
      }
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
  }, [active, blockedReason, plan, searchState, jump.term]);

  // ── Content pass (best-effort, silent on failure) ──
  useEffect(() => {
    if (contentTimerRef.current) clearTimeout(contentTimerRef.current);
    if (!active) return;
    if (blockedReason || jump.term.length < 2 || !contentSearchApplies(searchState)) {
      setContentRows([]);
      return;
    }
    let cancelled = false;
    const terms = buildContentTerms(searchState);
    // KT: the `in` chip must scope BOTH result sections. The content index only
    // started honouring a folder scope recently, and its command gained the
    // parameter later still — until this call passed it, an "in Downloads" chip
    // narrowed the filename list while "Inside files" below it kept showing hits
    // from the whole disk. Two lists visibly disagreeing about the same filter
    // is worse than having no filter.
    const scopePaths = splitScopePaths(chipOf(searchState, "in")?.path);
    const contentScope = scopePaths.length === 1 ? scopePaths[0] : undefined;
    contentTimerRef.current = setTimeout(() => {
      invoke<ContentHit[]>(
        "search_content",
        {
          ...buildContentQueryArgs(terms, 5),
          ...(contentScope ? { scope_path: contentScope } : {}),
        } as unknown as Record<string, unknown>,
      )
        .then((hits) => { if (!cancelled) setContentRows(hits.map(contentHitToDisplayRow)); })
        .catch(() => { /* the bar must not break when the index is cold */ });
    }, CONTENT_DEBOUNCE_MS);

    return () => {
      cancelled = true;
      if (contentTimerRef.current) clearTimeout(contentTimerRef.current);
    };
  }, [active, blockedReason, jump.term, searchState]);

  return {
    query, setQuery, suggestion,
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
