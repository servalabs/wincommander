// SPDX-License-Identifier: AGPL-3.0-or-later
// src/lib/searchQueryPlan.test.ts
// Coverage for the two query planners. The rules worth breaking a build over:
// a soft time chip must NOT emit a dm: filter (only a sort), folder scope must
// travel as scopePath and never as a token, and no Everything token may ever
// contain a space — a single spaced argv entry makes es.exe return zero rows.

import { describe, expect, it } from "bun:test";
import {
  buildContentTerms,
  buildEverythingPlan,
  contentSearchApplies,
  describeQuery,
  isDriveRootPath,
  splitScopePaths,
  toEverythingToken,
} from "./searchQueryPlan";
import { EMPTY_QUERY, addChip, cycleChipStrict } from "./searchTokens";
import type { ChipKind, QueryState } from "./searchTokens";

const q = (text: string): QueryState => ({ chips: [], text });
const withText = (state: QueryState, text: string): QueryState => ({ ...state, text });

/** Add a time chip and flip it to the hard-filter mode. */
function strictTime(state: QueryState, kind: ChipKind): QueryState {
  const added = addChip(state, kind);
  return cycleChipStrict(added, added.chips.findIndex((c) => c.kind === kind));
}

const chips = (...kinds: ChipKind[]): QueryState =>
  kinds.reduce<QueryState>((acc, kind) => addChip(acc, kind), EMPTY_QUERY);

// A fixed local wall-clock instant so the after: assertions are deterministic.
const NOW = new Date(2026, 6, 28, 23, 30, 0); // 28 Jul 2026, local

describe("buildEverythingPlan — scope and types", () => {
  it("emits folder: for a folders chip and file: for a files chip", () => {
    expect(buildEverythingPlan(chips("folders")).tokens).toContain("folder:");
    expect(buildEverythingPlan(chips("files")).tokens).toContain("file:");
  });

  it("emits neither when a hand-built state holds both (that combination returns zero rows)", () => {
    const both: QueryState = {
      chips: [{ kind: "files", source: "files" }, { kind: "folders", source: "folders" }],
      text: "x",
    };
    expect(buildEverythingPlan(both).tokens).toEqual(["*x*"]);
  });

  it("combines every type chip into ONE semicolon-separated ext: token", () => {
    const plan = buildEverythingPlan(chips("documents", "code", "images"));
    const extTokens = plan.tokens.filter((t) => t.startsWith("ext:"));
    expect(extTokens).toHaveLength(1);
    const exts = extTokens[0].slice(4).split(";");
    expect(exts).toContain("pdf");
    expect(exts).toContain("ts");
    expect(exts).toContain("png");
    // Deduped: the union carries each extension exactly once.
    expect(new Set(exts).size).toBe(exts.length);
  });

  it("orders the ext: set independently of the order chips were added", () => {
    const a = buildEverythingPlan(chips("code", "documents")).tokens;
    const b = buildEverythingPlan(chips("documents", "code")).tokens;
    expect(a).toEqual(b);
  });

  it("emits size and special tokens", () => {
    expect(buildEverythingPlan(chips("big")).tokens).toContain("size:>100mb");
    expect(buildEverythingPlan(chips("small")).tokens).toContain("size:<1mb");
    expect(buildEverythingPlan(chips("empty")).tokens).toContain("empty:");
    expect(buildEverythingPlan(chips("duplicates")).tokens).toContain("dupe:");
  });
});

describe("buildEverythingPlan — time chips are sort-biased by default", () => {
  it("emits NO dm: token when soft, but still sorts by recency", () => {
    const plan = buildEverythingPlan(withText(chips("thisWeek"), "budget"));
    expect(plan.tokens).toEqual(["*budget*"]);
    expect(plan.sort).toBe("dm-descending");
  });

  it("emits the dm: token when strict AND keeps the sort", () => {
    const plan = buildEverythingPlan(withText(strictTime(EMPTY_QUERY, "thisWeek"), "budget"));
    expect(plan.tokens).toContain("dm:thisweek");
    expect(plan.sort).toBe("dm-descending");
  });

  it("uses the verified es.exe token for each strict time chip", () => {
    const expected: [ChipKind, string][] = [
      ["today", "dm:today"],
      ["yesterday", "dm:yesterday"],
      ["thisWeek", "dm:thisweek"],
      ["last30Days", "dm:last30days"],
      ["thisYear", "dm:thisyear"],
    ];
    for (const [kind, token] of expected) {
      const plan = buildEverythingPlan(withText(strictTime(EMPTY_QUERY, kind), "x"));
      expect(plan.tokens).toContain(token);
    }
  });

  // Recency is the default sort for EVERY query, including a plain text search
  // with no time chip. es.exe's native order is by path, which buries the user's
  // own files behind C:\Windows and C:\Program Files: measured on a real box,
  // the developer's own D:\GitHub\assets ranked 1193rd of 1239 folders named
  // `assets*`, so it fell outside the fetched window entirely and no amount of
  // client-side ranking could recover it. dm-descending puts it first.
  it("sorts by recency for a plain text search, so path order cannot bury user files", () => {
    expect(buildEverythingPlan(q("budget")).sort).toBe("dm-descending");
  });

  it("keeps the recency sort while a strict time chip also hard-filters", () => {
    const plan = buildEverythingPlan(withText(strictTime(EMPTY_QUERY, "today"), "x"));
    expect(plan.sort).toBe("dm-descending");
    expect(plan.tokens).toContain("dm:today");
  });
});

describe("buildEverythingPlan — scopePath", () => {
  it("puts the in chip in scopePath and never in the tokens", () => {
    const state = withText(addChip(EMPTY_QUERY, "in", { path: "D:\\GitHub\\wincommander", pathLabel: "wincommander" }), "readme");
    const plan = buildEverythingPlan(state);
    expect(plan.scopePath).toBe("D:\\GitHub\\wincommander");
    expect(plan.tokens).toEqual(["*readme*"]);
    expect(plan.tokens).not.toContain("path:D:\\GitHub\\wincommander");
  });

  it("survives a path containing spaces intact", () => {
    const state = addChip(EMPTY_QUERY, "in", { path: "C:\\Users\\Admin\\My Documents\\Q4 Reports", pathLabel: "Q4 Reports" });
    const plan = buildEverythingPlan(withText(state, "summary"));
    expect(plan.scopePath).toBe("C:\\Users\\Admin\\My Documents\\Q4 Reports");
    for (const token of plan.tokens) expect(token).not.toContain(" ");
  });

  it("leaves scopePath undefined without an in chip", () => {
    expect(buildEverythingPlan(q("x")).scopePath).toBeUndefined();
  });

  it("ORs two drive roots as one grouping token and does not set scopePath", () => {
    const state = withText(addChip(EMPTY_QUERY, "in", { path: "C:\\|D:\\", pathLabel: "C: + D:" }), "readme");
    const plan = buildEverythingPlan(state);
    expect(plan.scopePath).toBeUndefined();
    expect(plan.tokens).toContain("<C:|D:>");
    expect(plan.tokens.some((t) => t.startsWith("path:"))).toBe(false);
  });

  it("ORs three drive roots as one grouping token and does not set scopePath", () => {
    const state = addChip(EMPTY_QUERY, "in", { path: "C:\\|D:\\|E:\\", pathLabel: "C: + D: + E:" });
    const plan = buildEverythingPlan(state);
    expect(plan.scopePath).toBeUndefined();
    expect(plan.tokens).toContain("<C:|D:|E:>");
    expect(plan.tokens.some((t) => t.startsWith("path:"))).toBe(false);
  });

  it("still uses scopePath for a single folder and emits no grouping token", () => {
    const state = addChip(EMPTY_QUERY, "in", { path: "D:\\GitHub\\wincommander", pathLabel: "wincommander" });
    const plan = buildEverythingPlan(state);
    expect(plan.scopePath).toBe("D:\\GitHub\\wincommander");
    expect(plan.tokens.some((t) => t.includes("|") || t.startsWith("<"))).toBe(false);
    expect(plan.tokens.some((t) => t.startsWith("path:"))).toBe(false);
  });
});

describe("scope path helpers", () => {
  it("recognises a drive letter with optional trailing slashes as a root", () => {
    expect(isDriveRootPath("C:")).toBe(true);
    expect(isDriveRootPath("C:\\")).toBe(true);
    expect(isDriveRootPath("D:\\\\")).toBe(true);
    expect(isDriveRootPath("D:\\GitHub\\wincommander")).toBe(false);
  });

  it("splits a pipe-joined path and leaves a single folder as one entry", () => {
    expect(splitScopePaths("C:\\|D:\\")).toEqual(["C:\\", "D:\\"]);
    expect(splitScopePaths("D:\\GitHub\\wincommander")).toEqual(["D:\\GitHub\\wincommander"]);
    expect(splitScopePaths(undefined)).toEqual([]);
  });
});

describe("buildEverythingPlan — text and browse", () => {
  it("splits multi-word text into one wrapped token per word (AND-of-substrings preserved)", () => {
    const plan = buildEverythingPlan(q("  budget   report q4 "));
    expect(plan.tokens).toEqual(["*budget*", "*report*", "*q4*"]);
    for (const token of plan.tokens) expect(token).not.toContain(" ");
  });

  it("never leaks the > folder-jump prefix into a token", () => {
    const plan = buildEverythingPlan(q(">assets"));
    expect(plan.tokens).toEqual(["*assets*"]);
  });

  it("produces a browse plan for the empty query", () => {
    const plan = buildEverythingPlan(EMPTY_QUERY);
    expect(plan.tokens).toEqual([]);
    expect(plan.isBrowse).toBe(true);
    expect(plan.sort).toBe("dm-descending");
    expect(plan.scopePath).toBeUndefined();
  });

  it("scopes the browse to the profile folder the caller supplied as an in chip", () => {
    const plan = buildEverythingPlan(addChip(EMPTY_QUERY, "in", { path: "C:\\Users\\Admin", pathLabel: "Admin" }));
    expect(plan.isBrowse).toBe(true);
    expect(plan.sort).toBe("dm-descending");
    expect(plan.scopePath).toBe("C:\\Users\\Admin");
  });

  it("is not a browse as soon as there is text", () => {
    expect(buildEverythingPlan(q("a")).isBrowse).toBe(false);
    expect(buildEverythingPlan(chips("images")).isBrowse).toBe(true);
  });

  it("orders filters before the search words", () => {
    const state = withText(strictTime(chips("folders"), "today"), "assets");
    expect(buildEverythingPlan(state).tokens).toEqual(["folder:", "dm:today", "*assets*"]);
  });
});

describe("buildEverythingPlan — operator/switch neutralisation and separator tolerance", () => {
  // Defect 1 (HIGH, 7.7% of the real corpus): a lone "-" from the common
  // "Name - Description" pattern (e.g. Windows' own WinX shortcuts,
  // `01a - Windows PowerShell.lnk`) must not survive as its own token — the
  // Rust guard rejects any token starting with "-" and errors the WHOLE query.
  it("drops a lone '-' word instead of emitting a token the backend guard rejects", () => {
    const plan = buildEverythingPlan(q("01a - Windows PowerShell.lnk"));
    expect(plan.tokens).toEqual(["*01a*", "*Windows*", "*PowerShell*lnk*"]);
    for (const token of plan.tokens) {
      expect(token.startsWith("-")).toBe(false);
      expect(token.startsWith("/")).toBe(false);
    }
  });

  it("drops other lone-operator words the same way (surviving '>' from 'A > B.txt', etc.)", () => {
    expect(toEverythingToken("-")).toBeNull();
    expect(toEverythingToken(">")).toBeNull();
    expect(toEverythingToken("<")).toBeNull();
    expect(toEverythingToken("|")).toBeNull();
    expect(toEverythingToken("...")).toBeNull();
    expect(toEverythingToken("___")).toBeNull();
  });

  // Defect 3 (HIGH, silent wrong answers): a leading "!" (also "|" "<" ">")
  // is read by es.exe as an operator (NOT/OR/grouping), not literal text.
  it("neutralises a leading '!' so it is literal text, not the NOT operator", () => {
    const plan = buildEverythingPlan(q("!Read"));
    expect(plan.tokens).toEqual(["*!Read*"]);
    expect(plan.tokens[0].startsWith("!")).toBe(false);
  });

  it("never emits a token starting with an operator/switch character for any leading symbol", () => {
    for (const symbol of ["-", "/", "!", "|", "<", ">"]) {
      const token = toEverythingToken(`${symbol}word`);
      expect(token).not.toBeNull();
      expect(token!.startsWith(symbol)).toBe(false);
      expect(token!.startsWith("*")).toBe(true);
    }
  });

  // Defect 2 (HIGH, 0% recall/268 cases): the user does not remember whether
  // a filename used a space, dash, underscore, or dot. Runs of any of those
  // WITHIN a word collapse to one wildcard, so all four spellings of
  // "Docker Desktop" produce an equivalent, separator-tolerant token.
  it("collapses -/_/. runs within a word into a single wildcard (separator tolerance)", () => {
    expect(toEverythingToken("Docker-Desktop")).toBe("*Docker*Desktop*");
    expect(toEverythingToken("Docker_Desktop")).toBe("*Docker*Desktop*");
    expect(toEverythingToken("Docker.Desktop")).toBe("*Docker*Desktop*");
    // Typed with a real space: two separate whitespace-split words, each
    // wrapped independently — still an AND of the two substrings.
    const plan = buildEverythingPlan(q("docker desktop"));
    expect(plan.tokens).toEqual(["*docker*", "*desktop*"]);
  });

  it("wrapping a plain word in */* is a no-op for Everything's default substring match, so it never regresses a query with no punctuation", () => {
    expect(toEverythingToken("budget")).toBe("*budget*");
    expect(toEverythingToken("report")).toBe("*report*");
  });

  it("never emits a token containing a raw space", () => {
    const plan = buildEverythingPlan(q("My Documents Q4"));
    for (const token of plan.tokens) expect(token).not.toContain(" ");
  });
});

describe("buildContentTerms", () => {
  it("puts the search text first, then filter tokens", () => {
    const terms = buildContentTerms(withText(chips("documents"), "budget report"), NOW);
    expect(terms.startsWith("budget report ")).toBe(true);
    expect(terms).toContain("ext:");
  });

  it("uses COMMAS where the Everything plan uses semicolons", () => {
    const state = chips("documents", "code");
    const contentExt = buildContentTerms(state, NOW).split(" ").filter((t) => t.startsWith("ext:"))[0];
    const everythingExt = buildEverythingPlan(state).tokens.filter((t) => t.startsWith("ext:"))[0];
    expect(contentExt).toContain(",");
    expect(contentExt).not.toContain(";");
    expect(everythingExt).toContain(";");
    expect(everythingExt).not.toContain(",");
    // Same extension set, different separator.
    expect(contentExt.slice(4).split(",")).toEqual(everythingExt.slice(4).split(";"));
  });

  it("emits no after: filter for a soft time chip", () => {
    const terms = buildContentTerms(withText(chips("today"), "budget"), NOW);
    expect(terms).toBe("budget");
  });

  it("emits an absolute local after: date for a strict time chip", () => {
    const at = (kind: ChipKind) => buildContentTerms(withText(strictTime(EMPTY_QUERY, kind), "x"), NOW);
    expect(at("today")).toBe("x after:2026-07-28");
    expect(at("yesterday")).toBe("x after:2026-07-27");
    expect(at("thisWeek")).toBe("x after:2026-07-21");
    expect(at("last30Days")).toBe("x after:2026-06-28");
    expect(at("thisYear")).toBe("x after:2026-01-01");
  });

  it("zero-pads and crosses a year boundary correctly", () => {
    const jan = new Date(2026, 0, 3, 9, 0, 0);
    expect(buildContentTerms(strictTime(EMPTY_QUERY, "thisWeek"), jan)).toBe("after:2025-12-27");
    expect(buildContentTerms(strictTime(EMPTY_QUERY, "today"), jan)).toBe("after:2026-01-03");
  });

  it("carries size filters and skips ext: when no type chip is active", () => {
    expect(buildContentTerms(withText(chips("big"), "iso"), NOW)).toBe("iso size:>100mb");
    expect(buildContentTerms(withText(chips("small"), "iso"), NOW)).toBe("iso size:<1mb");
  });

  it("returns just the text when nothing else applies", () => {
    expect(buildContentTerms(q("budget"), NOW)).toBe("budget");
    expect(buildContentTerms(EMPTY_QUERY, NOW)).toBe("");
  });
});

describe("contentSearchApplies", () => {
  it("is false for chip sets with nothing to read inside", () => {
    expect(contentSearchApplies(chips("folders"))).toBe(false);
    expect(contentSearchApplies(chips("empty"))).toBe(false);
    expect(contentSearchApplies(chips("duplicates"))).toBe(false);
    expect(contentSearchApplies(chips("images"))).toBe(false);
    expect(contentSearchApplies(chips("images", "videos", "audio"))).toBe(false);
    expect(contentSearchApplies(chips("apps"))).toBe(false);
    expect(contentSearchApplies(chips("archives"))).toBe(false);
  });

  it("is true whenever a text-bearing format is in play", () => {
    expect(contentSearchApplies(EMPTY_QUERY)).toBe(true);
    expect(contentSearchApplies(chips("files"))).toBe(true);
    expect(contentSearchApplies(chips("documents"))).toBe(true);
    expect(contentSearchApplies(chips("code"))).toBe(true);
    expect(contentSearchApplies(chips("images", "documents"))).toBe(true);
    expect(contentSearchApplies(chips("today", "big"))).toBe(true);
  });
});

describe("describeQuery", () => {
  it("describes the empty query as a recent-files browse", () => {
    expect(describeQuery(EMPTY_QUERY)).toBe("Recent files");
  });

  it("reads like a sentence for common combinations", () => {
    expect(describeQuery(withText(chips("folders", "today"), "assets")))
      .toBe("Folders named assets, changed today");
    expect(describeQuery(addChip(chips("images"), "in", { path: "C:\\Users\\Admin\\Downloads", pathLabel: "Downloads" })))
      .toBe("Images in Downloads");
    expect(describeQuery(addChip(EMPTY_QUERY, "in", { path: "C:\\|D:\\", pathLabel: "C: + D:" })))
      .toBe("Files and folders in C: and D:");
    expect(describeQuery(withText(chips("files", "big"), "iso")))
      .toBe("Files named iso, larger than 100 MB");
    expect(describeQuery(withText(chips("documents", "code"), "budget")))
      .toBe("Documents and code files named budget");
    expect(describeQuery(q("invoice"))).toBe("Files and folders named invoice");
  });

  it("folds empty / duplicates into the noun", () => {
    expect(describeQuery(chips("empty", "folders"))).toBe("Empty folders");
    expect(describeQuery(chips("duplicates", "files"))).toBe("Duplicate files");
  });

  it("names each time window and the small-size filter", () => {
    expect(describeQuery(chips("yesterday"))).toBe("Files and folders, changed yesterday");
    expect(describeQuery(chips("thisWeek"))).toBe("Files and folders, changed this week");
    expect(describeQuery(chips("last30Days"))).toBe("Files and folders, changed in the last 30 days");
    expect(describeQuery(chips("thisYear"))).toBe("Files and folders, changed this year");
    expect(describeQuery(chips("small"))).toBe("Files and folders, smaller than 1 MB");
  });

  it("reads the same whether a time chip is soft or strict", () => {
    const soft = withText(chips("today"), "assets");
    const strict = withText(strictTime(EMPTY_QUERY, "today"), "assets");
    expect(describeQuery(strict)).toBe(describeQuery(soft));
  });

  it("drops the > jump prefix from the description", () => {
    expect(describeQuery(q(">assets"))).toBe("Files and folders named assets");
  });
});
