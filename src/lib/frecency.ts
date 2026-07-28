// SPDX-License-Identifier: AGPL-3.0-or-later
// src/lib/frecency.ts
//
// "What you actually use" is the strongest ranking signal a launcher has —
// it's why Spotlight/Alfred feel psychic. This module remembers which paths
// the user opens (search results, recent files, anywhere an activation is
// recorded) and scores them for ranking. Pure localStorage, no Rust/IPC: the
// main window and the Ctrl+Space overlay share one WebView2 origin, so a
// plain `localStorage` write here is already visible to both.
//
// Every public function is defensive by construction — localStorage can be
// unavailable (SSR-ish contexts, WebView2 privacy modes) or throw (quota),
// and the persisted JSON can be corrupt, truncated, or written by a future
// schema version. None of that may ever propagate into a render path: every
// access is wrapped, and anything that doesn't match the expected shape is
// discarded in favour of "no history" rather than re-thrown.

export interface FrecencyEntry {
  path: string;
  opens: number;
  lastOpened: number;
}

const STORAGE_KEY = "wincmd.frecency.v1";
const STORAGE_VERSION = 1;
// KT: cap the store — localStorage is a synchronous, unbounded-write API with
// a small per-origin quota (~5-10MB in WebView2). Without a cap, years of
// "every file opened" would eventually blow the quota and every future write
// would silently start failing. 500 is comfortably more history than any
// ranking heuristic needs.
const MAX_ENTRIES = 500;

const DAY_MS = 24 * 60 * 60 * 1000;

// KT: time-BUCKET decay (Firefox's "frecency" algorithm), not `count / age`.
// Raw division makes a single very-recent open swamp everything else as age
// approaches zero (score approaches infinity), which makes ranking feel
// random rather than psychic. Buckets instead put a ceiling on how much any
// one recency tier is worth, so ranking is a stable, explainable ordering of
// "recent" vs. "stale" rather than a runaway function of exact age.
const RECENCY_BUCKETS: ReadonlyArray<{ maxAgeDays: number; weight: number }> = [
  { maxAgeDays: 4, weight: 100 },
  { maxAgeDays: 14, weight: 70 },
  { maxAgeDays: 31, weight: 50 },
  { maxAgeDays: 90, weight: 30 },
];
// Anything older than the last bucket still counts for something (a file you
// opened 40 times last year isn't nothing) but never competes with anything
// opened this week.
const STALE_WEIGHT = 10;

function recencyWeight(ageMs: number): number {
  const ageDays = Math.max(0, ageMs) / DAY_MS;
  for (const bucket of RECENCY_BUCKETS) {
    if (ageDays < bucket.maxAgeDays) return bucket.weight;
  }
  return STALE_WEIGHT;
}

// Open COUNT still matters — it's the tie-breaker within a recency bucket —
// but it's log-dampened so it can never overpower recency (see the "30 opens
// a year ago loses to 2 opens today" test). A raw linear count would let an
// old habit outrank something opened five minutes ago.
function computeScore(entry: FrecencyEntry, now: number): number {
  const ageMs = now - entry.lastOpened;
  const countFactor = 1 + Math.log2(Math.max(1, entry.opens));
  return recencyWeight(ageMs) * countFactor;
}

// Windows paths are case-insensitive and accept either separator, so
// "D:/x/y.txt" and "D:\X\Y.TXT" must resolve to the same history entry. The
// *display* casing (FrecencyEntry.path) is kept as whatever was last passed
// to recordOpen — only this lookup key is normalised.
function normalizeKey(path: string): string {
  return path.trim().replace(/\\/g, "/").toLowerCase();
}

function getStorage(): Storage | null {
  try {
    // `typeof` on an undeclared global never throws, unlike referencing it
    // directly — safe to probe for availability before touching it.
    if (typeof localStorage === "undefined") return null;
    return localStorage;
  } catch {
    return null;
  }
}

function isValidEntry(value: unknown): value is FrecencyEntry {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.path === "string" &&
    candidate.path.length > 0 &&
    typeof candidate.opens === "number" &&
    Number.isFinite(candidate.opens) &&
    candidate.opens > 0 &&
    typeof candidate.lastOpened === "number" &&
    Number.isFinite(candidate.lastOpened)
  );
}

/** Reads the store, keyed by normalised path. Any shape mismatch — corrupt
 * JSON, a future schema version, a tampered value — degrades to an empty
 * store rather than throwing or propagating junk entries. */
function readStore(): Record<string, FrecencyEntry> {
  const storage = getStorage();
  if (!storage) return {};

  let raw: string | null;
  try {
    raw = storage.getItem(STORAGE_KEY);
  } catch {
    return {};
  }
  if (!raw) return {};

  try {
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return {};
    const entriesRaw = (parsed as Record<string, unknown>).entries;
    if (typeof entriesRaw !== "object" || entriesRaw === null || Array.isArray(entriesRaw)) {
      return {};
    }
    const out: Record<string, FrecencyEntry> = {};
    for (const [key, value] of Object.entries(entriesRaw as Record<string, unknown>)) {
      // Validate per-entry rather than rejecting the whole file: a future
      // version may add fields we don't know about (fine, ignored below), but
      // a single hand-edited or truncated entry shouldn't poison the rest.
      if (isValidEntry(value)) out[key] = value;
    }
    return out;
  } catch {
    return {};
  }
}

function writeStore(store: Record<string, FrecencyEntry>): void {
  const storage = getStorage();
  if (!storage) return;
  try {
    storage.setItem(STORAGE_KEY, JSON.stringify({ v: STORAGE_VERSION, entries: store }));
  } catch {
    /* quota / disabled storage — non-fatal, matches notificationStore.ts */
  }
}

/** Drops the lowest-scoring entries in place until the store is back at the
 * cap. Score (not recency alone) decides who goes, so a handful of very
 * frequently opened old files survive longer than one-off recent opens. */
function enforceCap(store: Record<string, FrecencyEntry>, now: number): void {
  const keys = Object.keys(store);
  const overflow = keys.length - MAX_ENTRIES;
  if (overflow <= 0) return;

  keys
    .map((key) => ({ key, score: computeScore(store[key], now) }))
    .sort((a, b) => a.score - b.score)
    .slice(0, overflow)
    .forEach(({ key }) => delete store[key]);
}

/** Record that the user opened this path. Call on every activation. */
export function recordOpen(path: string, now: number = Date.now()): void {
  if (!path) return;

  const store = readStore();
  const key = normalizeKey(path);
  const existing = store[key];
  store[key] = {
    path,
    opens: (existing?.opens ?? 0) + 1,
    lastOpened: now,
  };
  enforceCap(store, now);
  writeStore(store);
}

/** Score for ranking: 0 when unknown, higher = more relevant. */
export function frecencyScore(path: string, now: number = Date.now()): number {
  if (!path) return 0;
  const entry = readStore()[normalizeKey(path)];
  return entry ? computeScore(entry, now) : 0;
}

/** Highest-scoring known paths, most relevant first. For the empty-state suggestions. */
export function topPaths(limit: number, now: number = Date.now()): string[] {
  if (limit <= 0) return [];
  return Object.values(readStore())
    .map((entry) => ({ path: entry.path, score: computeScore(entry, now) }))
    .sort((a, b) => b.score - a.score)
    .slice(0, limit)
    .map((entry) => entry.path);
}

/** Stable sort of candidates by descending frecency; ties keep input order. */
export function sortByFrecency<T>(
  items: T[],
  pathOf: (item: T) => string,
  now: number = Date.now(),
): T[] {
  // Decorate-sort-undecorate with the original index as an explicit
  // tie-breaker: guarantees stability regardless of what the ranking scores
  // to (equal scores are extremely common — most paths have no history at
  // all and all score 0) rather than leaning on engine sort-stability.
  return items
    .map((item, index) => ({ item, index, score: frecencyScore(pathOf(item), now) }))
    .sort((a, b) => (b.score !== a.score ? b.score - a.score : a.index - b.index))
    .map((entry) => entry.item);
}

export function clearFrecency(): void {
  const storage = getStorage();
  if (!storage) return;
  try {
    storage.removeItem(STORAGE_KEY);
  } catch {
    /* non-fatal */
  }
}
