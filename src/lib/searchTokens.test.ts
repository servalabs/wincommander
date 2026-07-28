// SPDX-License-Identifier: AGPL-3.0-or-later
// src/lib/searchTokens.test.ts
// Exhaustive coverage of the chip engine: catalogue invariants, the
// trailing-word suggestion pass (including the cases that must stay silent),
// and the promote / demote / cycle transitions. The demote round-trip tests are
// the load-bearing ones — a lossy undo is what makes users distrust the box.

import { describe, expect, it } from "bun:test";
import {
  CHIP_DEFS,
  EMPTY_QUERY,
  addChip,
  chipDef,
  chipOf,
  cycleChipStrict,
  demoteLastChip,
  parseFolderJump,
  promoteChip,
  removeChipAt,
  suggestChip,
} from "./searchTokens";
import type { ChipKind, ChipSuggestion, QueryState } from "./searchTokens";

const q = (text: string, chips: QueryState["chips"] = []): QueryState => ({ chips, text });

function mustSuggest(state: QueryState): ChipSuggestion {
  const s = suggestChip(state);
  if (!s) throw new Error(`expected a chip suggestion for ${JSON.stringify(state.text)}`);
  return s;
}

// Mirrors the verified icon names in src/components/ui/icon.tsx.
const VERIFIED_ICONS = [
  "folder-close", "folder-open", "document", "media", "video", "music", "compressed",
  "archive", "application", "code", "th", "layout-grid", "time", "calendar", "history",
  "stopwatch", "filter", "cross", "duplicate", "square", "properties", "database", "search",
];

// "duplicates" is a valid ChipKind (searchQueryPlan.ts still switches on it)
// but is deliberately NOT in this list — it has no CHIP_DEFS entry anymore
// (see the "duplicates — removed from the catalogue" suite below).
const ALL_KINDS: ChipKind[] = [
  "folders", "files", "images", "videos", "audio", "documents", "code", "archives", "apps",
  "today", "yesterday", "thisWeek", "last30Days", "thisYear", "big", "small",
  "empty", "duplicates", "in",
];

describe("CHIP_DEFS catalogue", () => {
  it("has exactly one def per kind and chipDef finds each", () => {
    expect(CHIP_DEFS).toHaveLength(ALL_KINDS.length);
    for (const kind of ALL_KINDS) expect(chipDef(kind).kind).toBe(kind);
  });

  it("uses only verified icon names and non-empty labels", () => {
    for (const def of CHIP_DEFS) {
      expect(VERIFIED_ICONS).toContain(def.icon);
      expect(def.label.length).toBeGreaterThan(0);
    }
  });

  it("never shares a trigger word between two kinds", () => {
    const owner = new Map<string, ChipKind>();
    for (const def of CHIP_DEFS) {
      for (const trigger of def.triggers) {
        expect(trigger).toBe(trigger.toLowerCase());
        expect(owner.get(trigger)).toBeUndefined();
        owner.set(trigger, def.kind);
      }
    }
  });

  it("marks supportsStrict on the five time chips only", () => {
    const strictKinds = CHIP_DEFS.filter((d) => d.supportsStrict).map((d) => d.kind);
    expect(strictKinds).toEqual(["today", "yesterday", "thisWeek", "last30Days", "thisYear"]);
  });
});

describe("suggestChip — trigger words", () => {
  it("maps every catalogued trigger to its own kind", () => {
    for (const def of CHIP_DEFS) {
      for (const trigger of def.triggers) {
        expect(mustSuggest(q(trigger)).chip.kind).toBe(def.kind);
      }
    }
  });

  it("accepts singular and plural forms of the same kind", () => {
    const pairs: [string, string, ChipKind][] = [
      ["folder", "folders", "folders"],
      ["file", "files", "files"],
      ["image", "images", "images"],
      ["video", "videos", "videos"],
      ["doc", "docs", "documents"],
      ["archive", "archives", "archives"],
      ["app", "apps", "apps"],
    ];
    for (const [singular, plural, kind] of pairs) {
      expect(mustSuggest(q(singular)).chip.kind).toBe(kind);
      expect(mustSuggest(q(plural)).chip.kind).toBe(kind);
    }
  });

  it("is case-insensitive but keeps the typed casing as the source", () => {
    const s = mustSuggest(q("FOLDERS"));
    expect(s.chip.kind).toBe("folders");
    expect(s.chip.source).toBe("FOLDERS");
    expect(s.consumed).toBe("FOLDERS");
  });

  it("completes an unambiguous partial word", () => {
    expect(mustSuggest(q("fol")).chip.kind).toBe("folders");
    expect(mustSuggest(q("fi")).chip.kind).toBe("files");
    expect(mustSuggest(q("mov")).chip.kind).toBe("videos");
    expect(mustSuggest(q("pho")).chip.kind).toBe("images");
  });

  it("stays silent on ambiguous or single-letter partials", () => {
    expect(suggestChip(q("f"))).toBeNull();   // file(s) vs folder(s)
    expect(suggestChip(q("mo"))).toBeNull();  // movie vs month
    expect(suggestChip(q("th"))).toBeNull();  // thisweek vs thismonth vs thisyear
    expect(suggestChip(q("d"))).toBeNull();
  });

  it("returns null for words that are not triggers at all", () => {
    expect(suggestChip(q("invoice"))).toBeNull();
    expect(suggestChip(q("documentation"))).toBeNull();
    expect(suggestChip(q(""))).toBeNull();
  });
});

describe("suggestChip — trailing word only", () => {
  it("suggests from the last word and hands back the rest of the text", () => {
    const s = mustSuggest(q("report files"));
    expect(s.chip.kind).toBe("files");
    expect(s.consumed).toBe("files");
    expect(s.nextText).toBe("report ");
  });

  it("ignores a trigger word that is not trailing", () => {
    expect(suggestChip(q("files report"))).toBeNull();
    expect(suggestChip(q("folder tree layout"))).toBeNull();
  });

  it("goes quiet once the user types a space after the word", () => {
    expect(suggestChip(q("folders "))).toBeNull();
  });

  it("returns null when that kind is already present", () => {
    const withFolders = addChip(EMPTY_QUERY, "folders");
    expect(suggestChip({ ...withFolders, text: "folders" })).toBeNull();
    expect(suggestChip({ ...withFolders, text: "dirs" })).toBeNull();
    // A different kind is still fair game.
    expect(mustSuggest({ ...withFolders, text: "today" }).chip.kind).toBe("today");
  });

  it("never mutates the state it was given", () => {
    const state = q("my folders");
    const s = mustSuggest(state);
    promoteChip(state, s);
    expect(state.text).toBe("my folders");
    expect(state.chips).toHaveLength(0);
  });
});

describe("promote / demote round-trip", () => {
  it("restores a plural trigger to the EXACT original text", () => {
    const before = q("my folders");
    const after = promoteChip(before, mustSuggest(before));
    expect(after.chips).toHaveLength(1);
    expect(after.chips[0].source).toBe("folders");
    expect(after.text).toBe("my ");
    const undone = demoteLastChip(after);
    expect(undone?.text).toBe("my folders");
    expect(undone?.chips).toHaveLength(0);
  });

  it("round-trips a lone trigger word with no leading text", () => {
    const before = q("dirs");
    const after = promoteChip(before, mustSuggest(before));
    expect(after.text).toBe("");
    expect(demoteLastChip(after)?.text).toBe("dirs");
  });

  it("never corrects the user's wording", () => {
    for (const typed of ["folders", "dirs", "pics", "songs", "Programs"]) {
      const after = promoteChip(q(typed), mustSuggest(q(typed)));
      expect(demoteLastChip(after)?.text).toBe(typed);
    }
  });

  it("keeps the restored word off the tail of existing text", () => {
    const state = { chips: [{ kind: "folders" as ChipKind, source: "folders" }], text: "assets" };
    expect(demoteLastChip(state)?.text).toBe("assets folders");
  });

  it("returns null when there are no chips to demote", () => {
    expect(demoteLastChip(EMPTY_QUERY)).toBeNull();
    expect(demoteLastChip(q("still typing"))).toBeNull();
  });

  it("demotes the LAST chip only", () => {
    const two = addChip(addChip(EMPTY_QUERY, "folders"), "today");
    const undone = demoteLastChip(two);
    expect(undone?.text).toBe("today");
    expect(undone?.chips).toHaveLength(1);
    expect(undone?.chips[0].kind).toBe("folders");
  });
});

describe("addChip", () => {
  it("defaults source to a trigger word so a clicked chip still demotes", () => {
    const state = addChip(EMPTY_QUERY, "documents");
    expect(state.chips[0].source).toBe("doc");
    expect(demoteLastChip(state)?.text).toBe("doc");
  });

  it("carries path + pathLabel for the in chip and labels its source", () => {
    const state = addChip(EMPTY_QUERY, "in", { path: "D:\\My Files\\assets", pathLabel: "assets" });
    expect(chipOf(state, "in")?.path).toBe("D:\\My Files\\assets");
    expect(state.chips[0].source).toBe("assets");
  });

  it("replaces an existing chip of the same kind in place", () => {
    const state = addChip(addChip(EMPTY_QUERY, "images"), "in", { path: "C:\\a", pathLabel: "a" });
    const next = addChip(state, "in", { path: "C:\\b", pathLabel: "b" });
    expect(next.chips).toHaveLength(2);
    expect(next.chips[1].pathLabel).toBe("b");
  });

  it("drops mutually exclusive siblings", () => {
    const scope = addChip(addChip(EMPTY_QUERY, "files"), "folders");
    expect(scope.chips.map((c) => c.kind)).toEqual(["folders"]);

    const size = addChip(addChip(EMPTY_QUERY, "big"), "small");
    expect(size.chips.map((c) => c.kind)).toEqual(["small"]);

    const time = addChip(addChip(addChip(EMPTY_QUERY, "today"), "thisWeek"), "thisYear");
    expect(time.chips.map((c) => c.kind)).toEqual(["thisYear"]);
  });

  it("keeps the in chip alongside a files/folders scope", () => {
    const state = addChip(addChip(EMPTY_QUERY, "folders"), "in", { path: "C:\\x", pathLabel: "x" });
    expect(state.chips).toHaveLength(2);
  });

  it("leaves the text untouched", () => {
    expect(addChip(q("budget"), "documents").text).toBe("budget");
  });
});

// Defect 2 (FIX-D, measured): Everything's `empty:` matches empty FOLDERS
// only, so pairing it with the `files` scope is a guaranteed, permanent zero.
describe("empty — relabelled and no longer combinable with files (Defect 2)", () => {
  it("is labelled 'Empty folders', not the old bare 'Empty'", () => {
    expect(chipDef("empty").label).toBe("Empty folders");
  });

  it("drops files when empty is added", () => {
    const state = addChip(addChip(EMPTY_QUERY, "files"), "empty");
    expect(state.chips.map((c) => c.kind)).toEqual(["empty"]);
  });

  it("drops empty when files is added", () => {
    const state = addChip(addChip(EMPTY_QUERY, "empty"), "files");
    expect(state.chips.map((c) => c.kind)).toEqual(["files"]);
  });

  it("still combines freely with folders — empty: genuinely only matches folders", () => {
    const state = addChip(addChip(EMPTY_QUERY, "folders"), "empty");
    expect(state.chips.map((c) => c.kind)).toEqual(["folders", "empty"]);
  });
});

// Defect 3 (FIX-D, measured): Everything's `dupe:` matches filename+size (not
// content) and a whole-disk click with no other input deterministically blows
// the 6-second search timeout, so the chip is retired from the catalogue.
// The ChipKind and searchQueryPlan.ts's `dupe:` planning stay intact — only
// the user-facing route to it (CHIP_DEFS + its trigger words) is gone.
// This chip was briefly retired for "exceeding the 6s timeout"; that timing was
// taken while seven load-testing agents shared one Everything daemon and did not
// reproduce (147-161ms over five trials on a quiet machine, 210ms at 5000 rows).
// What IS real and load-independent is the semantics: `dupe:` matches filename +
// size, never content. So the chip stays and the LABEL carries the caveat.
describe("duplicates — kept, but labelled for what the engine actually does", () => {
  it("is present in CHIP_DEFS", () => {
    expect(CHIP_DEFS.some((d) => d.kind === "duplicates")).toBe(true);
  });

  it("does not claim to find content duplicates", () => {
    const label = chipDef("duplicates").label;
    // "Duplicates" alone would promise content-level dedup the engine cannot do.
    expect(label.toLowerCase()).not.toBe("duplicates");
    expect(label).toBe("Same name & size");
  });

  it("still answers its trigger words", () => {
    for (const word of ["duplicate", "duplicates", "dupe", "dupes"]) {
      expect(suggestChip(q(word))?.chip.kind).toBe("duplicates");
    }
  });
});

describe("cycleChipStrict", () => {
  it("cycles a time chip soft -> strict -> removed", () => {
    const soft = addChip(EMPTY_QUERY, "today");
    expect(soft.chips[0].strict).toBeUndefined();
    const strict = cycleChipStrict(soft, 0);
    expect(strict.chips[0].strict).toBe(true);
    const gone = cycleChipStrict(strict, 0);
    expect(gone.chips).toHaveLength(0);
  });

  it("leaves non-time chips and bad indexes alone", () => {
    const state = addChip(EMPTY_QUERY, "images");
    expect(cycleChipStrict(state, 0)).toBe(state);
    expect(cycleChipStrict(state, 7)).toBe(state);
    expect(cycleChipStrict(state, -1)).toBe(state);
  });
});

describe("removeChipAt", () => {
  it("removes by index and ignores out-of-range indexes", () => {
    const state = addChip(addChip(EMPTY_QUERY, "folders"), "today");
    expect(removeChipAt(state, 0).chips.map((c) => c.kind)).toEqual(["today"]);
    expect(removeChipAt(state, 5)).toBe(state);
    expect(removeChipAt(state, -1)).toBe(state);
  });
});

describe("parseFolderJump", () => {
  it("detects the > jump prefix and strips it", () => {
    expect(parseFolderJump(">assets")).toEqual({ isJump: true, term: "assets" });
    expect(parseFolderJump("> assets")).toEqual({ isJump: true, term: "assets" });
    expect(parseFolderJump("  >assets  ")).toEqual({ isJump: true, term: "assets" });
    expect(parseFolderJump(">")).toEqual({ isJump: true, term: "" });
  });

  it("treats text without the prefix as a plain search", () => {
    expect(parseFolderJump("assets")).toEqual({ isJump: false, term: "assets" });
    expect(parseFolderJump("  budget report ")).toEqual({ isJump: false, term: "budget report" });
    expect(parseFolderJump("")).toEqual({ isJump: false, term: "" });
    expect(parseFolderJump("a > b")).toEqual({ isJump: false, term: "a > b" });
  });
});
