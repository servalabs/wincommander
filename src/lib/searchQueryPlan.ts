// SPDX-License-Identifier: AGPL-3.0-or-later
// src/lib/searchQueryPlan.ts
//
// Translates a chip QueryState (see searchTokens.ts) into what each search
// backend actually wants: an argv-shaped token plan for Everything's es.exe, a
// flat terms string for the tantivy content engine, and one line of plain
// English for the aria-live region and the empty state. Pure functions only.
//
// Two facts drive the whole file. (1) es.exe treats a single argv entry that
// contains a space as a quoted phrase, so every token here must be space-free
// and folder scope must travel as the -path FLAG, never as a `path:` token.
// (2) Time chips are SORT-BIASED by default: a user who says "last week" is
// about 60% sure, and a hard date filter turns a slightly wrong memory into an
// empty list that reads as "my file is gone".

import { FILE_TYPE_EXTENSIONS } from "@/lib/fileNameSearch";
import { chipOf, parseFolderJump } from "@/lib/searchTokens";
import type { Chip, ChipKind, QueryState } from "@/lib/searchTokens";

export interface EverythingPlan {
  /** Query tokens, each destined for its own es.exe argv entry. Never a bare space inside a token. */
  tokens: string[];
  /** Value for the backend `sort` param (e.g. "dm-descending"), or undefined. */
  sort?: string;
  /** Value for the backend `scopePath` param — passed as the -path FLAG, never as a token. */
  scopePath?: string;
  /** True when there is no text: a "recent files" browse rather than a search. */
  isBrowse: boolean;
}

/** Fixed order so the emitted ext: set is deterministic regardless of the order
 *  the user added the chips in. These names double as FILE_TYPE_EXTENSIONS keys. */
const TYPE_KINDS = ["documents", "images", "videos", "audio", "archives", "apps", "code"] as const;
type TypeKind = (typeof TYPE_KINDS)[number];

const TIME_KINDS = ["today", "yesterday", "thisWeek", "last30Days", "thisYear"] as const;
type TimeKind = (typeof TIME_KINDS)[number];

/** Verified es.exe date tokens — lowercase, no spaces. */
const DM_TOKENS: Record<TimeKind, string> = {
  today: "dm:today",
  yesterday: "dm:yesterday",
  thisWeek: "dm:thisweek",
  last30Days: "dm:last30days",
  thisYear: "dm:thisyear",
};

const TIME_PHRASES: Record<TimeKind, string> = {
  today: "changed today",
  yesterday: "changed yesterday",
  thisWeek: "changed this week",
  last30Days: "changed in the last 30 days",
  thisYear: "changed this year",
};

const TYPE_NOUNS: Record<TypeKind, string> = {
  documents: "documents",
  images: "images",
  videos: "videos",
  audio: "audio files",
  archives: "archives",
  apps: "apps",
  code: "code files",
};

/** Only these two type sets hold indexable prose — an ext: set of images or
 *  installers makes a content search pointless. */
const TEXT_BEARING: readonly TypeKind[] = ["documents", "code"];

function kindsOf(state: QueryState): Set<ChipKind> {
  return new Set(state.chips.map((c) => c.kind));
}

/** Union of every selected type set, deduped, in TYPE_KINDS order. */
function extsFor(kinds: Set<ChipKind>): string[] {
  const out = new Set<string>();
  for (const kind of TYPE_KINDS) {
    if (!kinds.has(kind)) continue;
    for (const ext of FILE_TYPE_EXTENSIONS[kind]) out.add(ext);
  }
  return Array.from(out);
}

/** The one active time chip (they are mutually exclusive), narrowed to TimeKind
 *  so the token / phrase lookups need no cast. */
function activeTime(state: QueryState): { kind: TimeKind; chip: Chip } | undefined {
  for (const kind of TIME_KINDS) {
    const chip = chipOf(state, kind);
    if (chip) return { kind, chip };
  }
  return undefined;
}

/** The search term with a `>` folder-jump prefix stripped. The ">" is UI
 *  grammar, not something es.exe or tantivy should ever see. */
function searchTerm(state: QueryState): string {
  return parseFolderJump(state.text).term;
}

/**
 * Turn ONE whitespace-delimited word of user-typed text into an es.exe argv
 * token that can never be misread as an operator or a command-line switch,
 * and that tolerates the user not remembering which separator a filename
 * actually used. Returns null when the word is pure punctuation and should
 * vanish rather than become a token.
 *
 * KT: es.exe reads `!` `|` `<` `>` as NOT/OR/grouping operators, and the
 * backend's argv-injection guard (`validate_es_tokens` in backend.rs)
 * rejects any token starting with `-` or `/` outright, erroring the WHOLE
 * search. A raw whitespace split hits both: the common "Name - Description"
 * pattern (Windows' own WinX shortcuts, e.g. `01a - Windows PowerShell.lnk`)
 * produces a lone "-" word — MEASURED 314/4066 real corpus entries (7.7%)
 * contain a standalone " - " and error on their own exact visible name — and
 * a word starting with `!` silently returns the WRONG answer instead of an
 * error: one real corpus file (`!Read this first!.txt`) returns 0 hits as
 * typed, and the single-word variant `!Read` returns 1,382,850 rows (raw
 * `es` measured) because `!` negates instead of matching literally.
 *
 * Wrapping every surviving word in a leading and trailing `*` wildcard fixes
 * both at once: the token then starts with `*`, never an operator or switch
 * character, and Everything's default match mode is already substring
 * search, so `*word*` returns the EXACT same rows as bare `word` (raw `es`
 * measured: "ead" and "*ead*" both 20216 rows; a two-word AND query and its
 * wrapped form both 34 rows with the same 6 target hits) — provably
 * additive, not a regression of the multi-word AND-of-substrings behaviour
 * that measured 88.1% recall@300.
 *
 * Runs of `-`/`_`/`.` WITHIN a word are collapsed to a single `*` before
 * wrapping, so "Docker-Desktop", "Docker_Desktop" and "Docker.Desktop" all
 * become "*Docker*Desktop*" and find "Docker Desktop.exe" — MEASURED 0%
 * recall/268 real cases before this change, fixed after (raw `es` verified).
 * This is the same trick `normalizeQuery` in fileNameSearch.ts already uses;
 * this file just never called it, which is how the regression happened.
 *
 * A word that collapses to nothing but wildcard/punctuation (a lone "-", or
 * a lone operator like ">" surviving from "A > B.txt") has no letters or
 * digits left and is dropped rather than emitted as a bare-operator token.
 */
export function toEverythingToken(word: string): string | null {
  const collapsed = word.replace(/[-_.]+/g, "*");
  const core = collapsed.replace(/^\*+/, "").replace(/\*+$/, "");
  if (!/[A-Za-z0-9]/.test(core)) return null;
  return `*${core}*`;
}

/**
 * Build the Everything (es.exe) plan. `now` is accepted for symmetry with
 * buildContentTerms but is deliberately unused: es.exe resolves `dm:` tokens
 * against its own clock, so there is no date arithmetic to do here.
 */
export function buildEverythingPlan(state: QueryState, _now?: Date): EverythingPlan {
  const kinds = kindsOf(state);
  const tokens: string[] = [];

  // Scope. Belt-and-braces against a hand-built state holding both: emitting
  // `file:` and `folder:` together returns zero rows with no error.
  if (kinds.has("folders") && !kinds.has("files")) tokens.push("folder:");
  if (kinds.has("files") && !kinds.has("folders")) tokens.push("file:");

  const exts = extsFor(kinds);
  if (exts.length > 0) tokens.push(`ext:${exts.join(";")}`); // SEMICOLON for Everything

  if (kinds.has("big")) tokens.push("size:>100mb");
  if (kinds.has("small")) tokens.push("size:<1mb");

  const time = activeTime(state);
  // Soft (the default) contributes the sort only — no dm: token, so a memory
  // that is a few days off still finds the file, just further down.
  if (time?.chip.strict === true) tokens.push(DM_TOKENS[time.kind]);

  if (kinds.has("empty")) tokens.push("empty:");
  if (kinds.has("duplicates")) tokens.push("dupe:");

  // Each word becomes its own argv entry, so "budget report" means both words
  // appear (Everything substring-matches by default) rather than the literal
  // phrase — and no token can ever contain a space. toEverythingToken() also
  // neutralises operator/switch characters and tolerates separator mismatch;
  // see its doc comment for the measured defects this fixes.
  const term = searchTerm(state);
  for (const word of term.split(/\s+/)) {
    if (!word) continue;
    const token = toEverythingToken(word);
    if (token) tokens.push(token);
  }

  const isBrowse = term === "";
  const plan: EverythingPlan = { tokens, isBrowse };
  // KT: recency is the DEFAULT sort for every query, not just time-chip ones.
  // es.exe's native order is by path, which buries the user's own files behind
  // system noise: of the 1239 folders named `assets*` on this dev box, the
  // developer's own D:\GitHub\assets ranked 1193rd, because C:\Windows and
  // C:\Program Files sort before D:. Since we only fetch a window of rows and
  // rank client-side, that folder was never even in the fetched set — the
  // classic "fetch N then sort" trap. `-sort dm-descending` moves it to rank 1,
  // because system folders were written at OS install and the files you are
  // hunting for are the ones you touched recently. Same principle as the soft
  // time chip: prefer recent rather than filter on it.
  plan.sort = "dm-descending";
  const scope = chipOf(state, "in");
  if (scope?.path) plan.scopePath = scope.path;
  return plan;
}

/** Zero-padded YYYY-MM-DD from LOCAL date parts — a user in UTC+5:30 expects
 *  "today" to start at their own midnight. Mirrors contentQueryFilters.ts,
 *  whose copy is module-private. */
function formatLocalDate(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

function afterDateFor(kind: TimeKind, now: Date): string {
  const d = new Date(now);
  if (kind === "thisYear") return `${d.getFullYear()}-01-01`;
  if (kind === "yesterday") d.setDate(d.getDate() - 1);
  if (kind === "thisWeek") d.setDate(d.getDate() - 7);
  if (kind === "last30Days") d.setDate(d.getDate() - 30);
  return formatLocalDate(d);
}

/**
 * Terms string for the CONTENT backend (search_content). Different syntax from
 * Everything: `ext:` is COMMA separated and dates are absolute `after:`.
 *
 * KT: a soft time chip emits nothing here. Its whole meaning is "bias the
 * ranking", and the content backend ranks by relevance with no date input — so
 * turning it into an `after:` filter would quietly make it the hard filter the
 * default is designed to avoid. Only `strict` dates reach this backend.
 */
export function buildContentTerms(state: QueryState, now: Date = new Date()): string {
  const parts: string[] = [];
  const term = searchTerm(state);
  if (term) parts.push(term);

  const kinds = kindsOf(state);
  const exts = extsFor(kinds);
  if (exts.length > 0) parts.push(`ext:${exts.join(",")}`); // COMMA for filters.rs

  if (kinds.has("big")) parts.push("size:>100mb");
  if (kinds.has("small")) parts.push("size:<1mb");

  const time = activeTime(state);
  if (time?.chip.strict === true) parts.push(`after:${afterDateFor(time.kind, now)}`);

  return parts.join(" ");
}

/** False when the active chips make a content search pointless — folders-only
 *  scope, empty:, dupe:, or a type set with no text-bearing formats. */
export function contentSearchApplies(state: QueryState): boolean {
  const kinds = kindsOf(state);
  if (kinds.has("folders")) return false;   // folders have no text inside them
  if (kinds.has("empty")) return false;     // an empty file has nothing to match
  if (kinds.has("duplicates")) return false; // dupe: is a filename-level idea
  const types = TYPE_KINDS.filter((k) => kinds.has(k));
  if (types.length > 0 && !types.some((k) => TEXT_BEARING.includes(k))) return false;
  return true;
}

/** "images, videos and audio files" */
function joinList(items: string[]): string {
  if (items.length <= 1) return items[0] ?? "";
  return `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`;
}

/**
 * Plain-English rendering for the aria-live region and the empty state, e.g.
 * "Folders named assets, changed today". Used verbatim in the UI, so it reads
 * like a sentence a person would say — never a dump of the token plan.
 */
export function describeQuery(state: QueryState): string {
  const term = searchTerm(state);
  if (state.chips.length === 0 && term === "") return "Recent files";

  const kinds = kindsOf(state);
  const nouns = TYPE_KINDS.filter((k) => kinds.has(k)).map((k) => TYPE_NOUNS[k]);
  let head = joinList(nouns);
  if (!head) head = kinds.has("folders") ? "folders" : kinds.has("files") ? "files" : "files and folders";
  if (kinds.has("empty")) head = `empty ${head}`;
  if (kinds.has("duplicates")) head = `duplicate ${head}`;

  let out = head;
  if (term) out += ` named ${term}`;
  const scope = chipOf(state, "in");
  if (scope) out += ` in ${scope.pathLabel ?? scope.path ?? "this folder"}`;

  const clauses: string[] = [];
  const time = activeTime(state);
  if (time) clauses.push(TIME_PHRASES[time.kind]);
  if (kinds.has("big")) clauses.push("larger than 100 MB");
  if (kinds.has("small")) clauses.push("smaller than 1 MB");
  if (clauses.length > 0) out += `, ${clauses.join(", ")}`;

  return out.charAt(0).toUpperCase() + out.slice(1);
}
