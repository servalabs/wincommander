// SPDX-License-Identifier: AGPL-3.0-or-later
// Pure helpers for the Search Files panel's keyboard-first result list:
// a flat selection model spanning both result groups, query-token
// highlighting for file names, and the "Show more" limit ladder.
// No Tauri IPC here — all functions are pure transforms on plain data.

import type { SnippetSegment } from "./contentSearch";

/** One selectable row in the unified results list, in render order. */
export interface SelectionEntry {
  kind: "name" | "content";
  /** Index into the corresponding results array. */
  index: number;
}

/** Render order is: all filename rows, then all content rows. */
export function buildSelectionEntries(nameCount: number, contentCount: number): SelectionEntry[] {
  const entries: SelectionEntry[] = [];
  for (let i = 0; i < nameCount; i++) entries.push({ kind: "name", index: i });
  for (let i = 0; i < contentCount; i++) entries.push({ kind: "content", index: i });
  return entries;
}

/**
 * Move the virtual selection by `delta`. -1 means "nothing selected";
 * ArrowDown from nothing lands on the first row, ArrowUp from nothing on
 * the last. Movement clamps at the ends (no wrap — wrapping makes long
 * lists disorienting).
 */
export function stepSelection(current: number, delta: number, total: number): number {
  if (total <= 0) return -1;
  if (current < 0) return delta >= 0 ? 0 : total - 1;
  const next = current + delta;
  if (next < 0) return 0;
  if (next > total - 1) return total - 1;
  return next;
}

/**
 * True when the rendered result rows belong to the CURRENT query text.
 * Both searches are debounced, so right after a keystroke the rows on
 * screen still belong to the previous query — Enter must re-search
 * instead of opening a stale row (`resultsQuery`/`contentQuery` are the
 * raw query strings the current rows were fetched for). Content rows
 * only count once the query reaches the 2-char content-search gate.
 */
export function areResultsFresh(
  query: string,
  resultsQuery: string | null,
  contentQuery: string | null,
): boolean {
  if (resultsQuery !== query) return false;
  if (query.trim().length >= 2 && contentQuery !== query) return false;
  return true;
}

// ---------------------------------------------------------------------------
// File-name match highlighting
// ---------------------------------------------------------------------------

/** Everything-syntax operators that aren't literal text to highlight. */
const SYNTAX_TOKEN = /^(file:|folder:|ext:|size:|dm:|dc:|da:|regex:|path:|parent:)/i;

/**
 * Extract the plain text tokens a user would expect to see highlighted.
 * Splits on whitespace and the wildcard/separator characters the panel's
 * `normalizeQuery` folds into `*`; drops Everything operators and
 * single-character fragments (too noisy to highlight).
 */
export function extractHighlightTokens(query: string): string[] {
  const seen = new Set<string>();
  const tokens: string[] = [];
  // Whitespace split FIRST so an operator like `size:>10mb` is dropped whole
  // (splitting on `>` first would leak "10mb" as a highlight token).
  for (const raw of query.split(/\s+/)) {
    if (!raw || SYNTAX_TOKEN.test(raw)) continue;
    for (const part of raw.split(/[*?"<>|,.-]+/)) {
      const t = part.toLowerCase();
      if (t.length < 2 || seen.has(t)) continue;
      seen.add(t);
      tokens.push(t);
    }
  }
  // Longest first so "invoices" wins over "invoice" at the same position.
  return tokens.sort((a, b) => b.length - a.length);
}

function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Split a file name into segments, marking the parts that match any query
 * token so the UI can show *why* a row matched. Falls back to one
 * unhighlighted segment when the query has no usable tokens.
 */
export function highlightName(name: string, query: string): SnippetSegment[] {
  const tokens = extractHighlightTokens(query);
  if (tokens.length === 0 || name.length === 0) {
    return name ? [{ text: name, highlighted: false }] : [];
  }
  const re = new RegExp(`(${tokens.map(escapeRegExp).join("|")})`, "gi");
  const segs: SnippetSegment[] = [];
  let last = 0;
  for (const m of name.matchAll(re)) {
    const start = m.index ?? 0;
    if (start > last) segs.push({ text: name.slice(last, start), highlighted: false });
    segs.push({ text: m[0], highlighted: true });
    last = start + m[0].length;
  }
  if (last < name.length) segs.push({ text: name.slice(last), highlighted: false });
  return segs;
}

// ---------------------------------------------------------------------------
// "Show more" limit ladder
// ---------------------------------------------------------------------------

export const DEFAULT_RESULT_LIMIT = 200;
export const MIN_RESULT_LIMIT = 50;
export const MAX_RESULT_LIMIT = 2_000;
export const RESULT_LIMIT_LADDER = [50, 100, 200, 500, 1000, 2000] as const;

/** Keep a persisted or policy-provided search limit within the backend's
 * bounded result window. Invalid legacy values retain the shipped default. */
export function normalizeResultLimit(value: number | null | undefined): number {
  if (!Number.isFinite(value)) return DEFAULT_RESULT_LIMIT;
  return Math.min(MAX_RESULT_LIMIT, Math.max(MIN_RESULT_LIMIT, Math.floor(value as number)));
}

/** Next rung above `current`, or null when already at (or past) the top. */
export function nextResultLimit(current: number): number | null {
  for (const rung of RESULT_LIMIT_LADDER) {
    if (rung > current) return rung;
  }
  return null;
}
