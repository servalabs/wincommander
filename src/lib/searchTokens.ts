// SPDX-License-Identifier: AGPL-3.0-or-later
// src/lib/searchTokens.ts
//
// The chip model behind the unified search box. Chips are the pills that sit
// immediately left of the text caret ("Folders", "Today", "In Downloads"). This
// module owns the catalogue of chip kinds, the trailing-word → suggestion pass
// that offers one, and the promote / demote / cycle transitions over a
// QueryState. Everything here is pure — the UI owns the state, these functions
// only ever return a new value. Translating a QueryState into the two backends'
// query syntax lives next door in searchQueryPlan.ts.

export type ChipKind =
  | "folders" | "files"
  | "images" | "videos" | "audio" | "documents" | "code" | "archives" | "apps"
  | "today" | "yesterday" | "thisWeek" | "last30Days" | "thisYear"
  | "big" | "small"
  | "empty" | "duplicates"
  | "in";

export type ChipGroup = "scope" | "type" | "time" | "size" | "special";

export interface Chip {
  kind: ChipKind;
  /** VERBATIM text the user typed that produced this chip. Restored exactly on
   *  demote — if they typed "folders" they get "folders" back, never "folder". */
  source: string;
  /** kind==="in" only: absolute folder path. */
  path?: string;
  /** kind==="in" only: short display label, e.g. "Downloads". */
  pathLabel?: string;
  /** Time chips only. false/undefined = rank bias only (the DEFAULT). true = hard filter. */
  strict?: boolean;
}

export interface ChipDef {
  kind: ChipKind;
  label: string;
  icon: string;
  group: ChipGroup;
  /** Lowercase trigger words, longest match wins. */
  triggers: string[];
  /** True for the five time chips — the only ones with a soft/strict mode. */
  supportsStrict?: boolean;
}

// Icons are names from the verified Lucide map in src/components/ui/icon.tsx.
// `big` and `small` deliberately share one icon: they are the same question.
export const CHIP_DEFS: readonly ChipDef[] = [
  { kind: "folders",    label: "Folders",      icon: "folder-close", group: "scope",   triggers: ["folder", "folders", "dir", "dirs", "directory"] },
  { kind: "files",      label: "Files",        icon: "th",           group: "scope",   triggers: ["file", "files"] },
  { kind: "in",         label: "In",           icon: "folder-open",  group: "scope",   triggers: [] },
  { kind: "images",     label: "Images",       icon: "media",        group: "type",    triggers: ["image", "images", "img", "pic", "pics", "picture", "pictures", "photo", "photos"] },
  { kind: "videos",     label: "Videos",       icon: "video",        group: "type",    triggers: ["video", "videos", "movie", "movies"] },
  { kind: "audio",      label: "Audio",        icon: "music",        group: "type",    triggers: ["audio", "music", "song", "songs", "sound"] },
  { kind: "documents",  label: "Documents",    icon: "document",     group: "type",    triggers: ["doc", "docs", "document", "documents"] },
  { kind: "code",       label: "Code",         icon: "code",         group: "type",    triggers: ["code", "source"] },
  { kind: "archives",   label: "Archives",     icon: "compressed",   group: "type",    triggers: ["archive", "archives", "zip", "zips"] },
  { kind: "apps",       label: "Apps",         icon: "application",  group: "type",    triggers: ["app", "apps", "application", "applications", "program", "programs", "exe"] },
  { kind: "today",      label: "Today",        icon: "time",         group: "time",    triggers: ["today"], supportsStrict: true },
  { kind: "yesterday",  label: "Yesterday",    icon: "history",      group: "time",    triggers: ["yesterday"], supportsStrict: true },
  { kind: "thisWeek",   label: "This week",    icon: "calendar",     group: "time",    triggers: ["week", "thisweek"], supportsStrict: true },
  { kind: "last30Days", label: "Last 30 days", icon: "calendar",     group: "time",    triggers: ["month", "thismonth", "recent"], supportsStrict: true },
  { kind: "thisYear",   label: "This year",    icon: "calendar",     group: "time",    triggers: ["year", "thisyear"], supportsStrict: true },
  { kind: "big",        label: "Big",          icon: "database",     group: "size",    triggers: ["big", "large", "huge"] },
  { kind: "small",      label: "Small",        icon: "database",     group: "size",    triggers: ["small", "tiny"] },
  { kind: "empty",      label: "Empty",        icon: "square",       group: "special",  triggers: ["empty"] },
  { kind: "duplicates", label: "Duplicates",   icon: "duplicate",    group: "special",  triggers: ["duplicate", "duplicates", "dupe", "dupes"] },
];

const DEFS_BY_KIND: Map<ChipKind, ChipDef> = new Map(CHIP_DEFS.map((d) => [d.kind, d]));

export function chipDef(kind: ChipKind): ChipDef {
  const def = DEFS_BY_KIND.get(kind);
  // Unreachable through the type system; a throw beats returning a fake def
  // that would silently render a chip with no label.
  if (!def) throw new Error(`unknown chip kind: ${kind}`);
  return def;
}

export interface QueryState { chips: Chip[]; text: string; }
export const EMPTY_QUERY: QueryState = { chips: [], text: "" };

export interface ChipSuggestion { chip: Chip; consumed: string; nextText: string; }

/** Chips that cannot coexist — adding one drops its siblings. `file: folder:`
 *  in a single Everything query silently returns zero rows, and "modified today
 *  AND yesterday" / "big AND small" are answerless in the same way. `in` is in
 *  the scope group but combines freely with files/folders. */
const EXCLUSIVE_SETS: readonly ChipKind[][] = [
  ["files", "folders"],
  ["big", "small"],
  ["today", "yesterday", "thisWeek", "last30Days", "thisYear"],
];

function exclusiveSiblings(kind: ChipKind): Set<ChipKind> {
  const out = new Set<ChipKind>();
  for (const set of EXCLUSIVE_SETS) {
    if (!set.includes(kind)) continue;
    for (const k of set) if (k !== kind) out.add(k);
  }
  return out;
}

/** First chip of `kind`, or undefined. Only one chip per kind ever exists. */
export function chipOf(state: QueryState, kind: ChipKind): Chip | undefined {
  return state.chips.find((c) => c.kind === kind);
}

/** Shortest trigger word is the friendliest fallback `source` for a chip that
 *  was clicked rather than typed; `in` has no trigger so it uses its label. */
function defaultSource(def: ChipDef, extra?: Partial<Chip>): string {
  if (def.kind === "in") return extra?.pathLabel ?? def.label.toLowerCase();
  return def.triggers[0] ?? def.label.toLowerCase();
}

function withChip(state: QueryState, chip: Chip, text: string): QueryState {
  const drop = exclusiveSiblings(chip.kind);
  const at = state.chips.findIndex((c) => c.kind === chip.kind);
  // Re-adding an existing kind replaces it IN PLACE (an `in` chip picking a new
  // folder must not make the pill row reshuffle under the pointer).
  const next = at >= 0 ? state.chips.map((c, i) => (i === at ? chip : c)) : [...state.chips, chip];
  return { chips: next.filter((c) => c === chip || !drop.has(c.kind)), text };
}

/** The trailing whitespace-delimited word plus everything before it. Null when
 *  the text is empty or already ends in whitespace — a typed space means the
 *  user moved on, so no suggestion is offered for the word they just finished. */
function trailingWord(text: string): { word: string; head: string } | null {
  const m = /\S+$/.exec(text);
  if (!m) return null;
  return { word: m[0], head: text.slice(0, m.index) };
}

/** Below this length a partial word is a coin flip, not a completion. */
const MIN_PREFIX = 2;

/**
 * Suggest a chip from the TRAILING word of state.text. Returns null when
 * nothing matches, when that chip kind is already present, or when the trailing
 * word is still being typed in a way that is ambiguous. Never mutates.
 *
 * Only the trailing word is ever considered: "report files" offers a chip,
 * "files report" does not, because there the word is part of what they are
 * looking for. The result is a *candidate* — the UI renders it greyed out and
 * the user confirms with Tab. Nothing here consumes a word on its own.
 */
export function suggestChip(state: QueryState): ChipSuggestion | null {
  const tail = trailingWord(state.text);
  if (!tail) return null;
  const word = tail.word.toLowerCase();

  // A fully typed trigger word wins outright; longest trigger breaks ties.
  let best: { def: ChipDef; trigger: string } | null = null;
  for (const def of CHIP_DEFS) {
    for (const trigger of def.triggers) {
      if (trigger !== word) continue;
      if (!best || trigger.length > best.trigger.length) best = { def, trigger };
    }
  }

  if (!best && word.length >= MIN_PREFIX) {
    // Ghost completion: offer a chip only when exactly ONE kind is still
    // reachable from this prefix. "mov" → Videos; "mo" could be Videos or
    // Last 30 days, and guessing there is what makes the box feel possessed.
    const kinds = new Set<ChipKind>();
    let candidate: { def: ChipDef; trigger: string } | null = null;
    for (const def of CHIP_DEFS) {
      for (const trigger of def.triggers) {
        if (!trigger.startsWith(word)) continue;
        kinds.add(def.kind);
        if (!candidate || trigger.length < candidate.trigger.length) candidate = { def, trigger };
      }
    }
    if (kinds.size === 1) best = candidate;
  }

  if (!best) return null;
  if (chipOf(state, best.def.kind)) return null;
  return {
    chip: { kind: best.def.kind, source: tail.word },
    consumed: tail.word,
    nextText: tail.head,
  };
}

/** Accept a suggestion: the chip joins the row, its word leaves the text. */
export function promoteChip(state: QueryState, s: ChipSuggestion): QueryState {
  return withChip(state, s.chip, s.nextText);
}

/** Add a chip the user clicked rather than typed. `extra` carries `path` /
 *  `pathLabel` for `in`, and may override `source`. */
export function addChip(state: QueryState, kind: ChipKind, extra?: Partial<Chip>): QueryState {
  const def = chipDef(kind);
  const chip: Chip = { ...extra, kind, source: extra?.source ?? defaultSource(def, extra) };
  return withChip(state, chip, state.text);
}

export function removeChipAt(state: QueryState, i: number): QueryState {
  if (i < 0 || i >= state.chips.length) return state;
  return { chips: state.chips.filter((_, idx) => idx !== i), text: state.text };
}

/**
 * Backspace with the caret at offset 0: the last chip becomes its `source` text
 * again. Returns null when there are no chips. LOSSLESS — this is an undo, not
 * a delete, so the word comes back exactly as it was typed.
 *
 * KT: the word is appended (the caret then sits after it, ready to be edited),
 * and a separating space is inserted only when the surviving text does not
 * already end in one. `promoteChip` keeps the original spacing in `nextText`,
 * so promote → demote reproduces the input string character for character.
 */
export function demoteLastChip(state: QueryState): QueryState | null {
  const last = state.chips[state.chips.length - 1];
  if (!last) return null;
  const needsGap = state.text.length > 0 && !/\s$/.test(state.text);
  return {
    chips: state.chips.slice(0, -1),
    text: needsGap ? `${state.text} ${last.source}` : state.text + last.source,
  };
}

/** Time chips cycle soft -> strict -> removed. Other kinds are always hard
 *  filters, so there is nothing to cycle and the state is returned untouched. */
export function cycleChipStrict(state: QueryState, i: number): QueryState {
  const chip = state.chips[i];
  if (!chip || !chipDef(chip.kind).supportsStrict) return state;
  if (chip.strict === true) return removeChipAt(state, i);
  return {
    chips: state.chips.map((c, idx) => (idx === i ? { ...c, strict: true } : c)),
    text: state.text,
  };
}

/** ">assets" means jump into the folder rather than list matches. */
export function parseFolderJump(text: string): { isJump: boolean; term: string } {
  const trimmed = text.trim();
  if (trimmed.startsWith(">")) return { isJump: true, term: trimmed.slice(1).trim() };
  return { isJump: false, term: trimmed };
}
