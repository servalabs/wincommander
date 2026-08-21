import { describe, expect, it } from "bun:test";
import {
  areResultsFresh,
  buildSelectionEntries,
  extractHighlightTokens,
  highlightName,
  normalizeResultLimit,
  nextResultLimit,
  stepSelection,
} from "./searchSelection";

describe("buildSelectionEntries", () => {
  it("orders name rows before content rows", () => {
    const entries = buildSelectionEntries(2, 2);
    expect(entries).toEqual([
      { kind: "name", index: 0 },
      { kind: "name", index: 1 },
      { kind: "content", index: 0 },
      { kind: "content", index: 1 },
    ]);
  });

  it("handles empty groups", () => {
    expect(buildSelectionEntries(0, 0)).toEqual([]);
    expect(buildSelectionEntries(0, 1)).toEqual([{ kind: "content", index: 0 }]);
  });
});

describe("stepSelection", () => {
  it("returns -1 when the list is empty", () => {
    expect(stepSelection(-1, 1, 0)).toBe(-1);
    expect(stepSelection(3, -1, 0)).toBe(-1);
  });

  it("enters the list at the first row on ArrowDown from nothing", () => {
    expect(stepSelection(-1, 1, 5)).toBe(0);
  });

  it("enters the list at the last row on ArrowUp from nothing", () => {
    expect(stepSelection(-1, -1, 5)).toBe(4);
  });

  it("moves and clamps at both ends without wrapping", () => {
    expect(stepSelection(0, 1, 3)).toBe(1);
    expect(stepSelection(2, 1, 3)).toBe(2);
    expect(stepSelection(0, -1, 3)).toBe(0);
  });

  it("clamps a stale index after the list shrank", () => {
    expect(stepSelection(9, 1, 3)).toBe(2);
  });
});

describe("areResultsFresh", () => {
  it("is stale while the filename debounce hasn't landed (Enter must re-search, not open)", () => {
    expect(areResultsFresh("budget2", "budget", "budget2")).toBe(false);
  });

  it("is stale while the content debounce hasn't landed", () => {
    expect(areResultsFresh("budget2", "budget2", "budget")).toBe(false);
  });

  it("is fresh when both result sets match the current query", () => {
    expect(areResultsFresh("budget2", "budget2", "budget2")).toBe(true);
  });

  it("ignores content freshness under the 2-char content-search gate", () => {
    expect(areResultsFresh("a", "a", null)).toBe(true);
    expect(areResultsFresh(" b ", " b ", "old")).toBe(true);
  });

  it("is stale before any search has landed", () => {
    expect(areResultsFresh("q!", null, null)).toBe(false);
  });
});

describe("extractHighlightTokens", () => {
  it("splits on whitespace and wildcards", () => {
    expect(extractHighlightTokens("budget report*2026")).toEqual([
      "budget",
      "report",
      "2026",
    ]);
  });

  it("drops Everything syntax operators and single chars", () => {
    expect(extractHighlightTokens("ext:pdf size:>10mb invoice a")).toEqual(["invoice"]);
  });

  it("splits separator-normalized fragments and dedupes, longest first", () => {
    expect(extractHighlightTokens("tax-tax invoice.pdf")).toEqual(["invoice", "tax", "pdf"]);
  });

  it("returns nothing for an all-operator query", () => {
    expect(extractHighlightTokens("file: dm:today")).toEqual([]);
  });
});

describe("highlightName", () => {
  it("marks case-insensitive matches of every token", () => {
    const segs = highlightName("Budget-REPORT-final.pdf", "budget report");
    expect(segs).toEqual([
      { text: "Budget", highlighted: true },
      { text: "-", highlighted: false },
      { text: "REPORT", highlighted: true },
      { text: "-final.pdf", highlighted: false },
    ]);
  });

  it("matches partial tokens inside the name (the 2982 case)", () => {
    const segs = highlightName("IMG_XXXXXXXX2982.jpg", "2982");
    expect(segs).toEqual([
      { text: "IMG_XXXXXXXX", highlighted: false },
      { text: "2982", highlighted: true },
      { text: ".jpg", highlighted: false },
    ]);
  });

  it("escapes regex metacharacters in tokens", () => {
    const segs = highlightName("notes(v2).txt", "(v2)");
    expect(segs).toEqual([
      { text: "notes", highlighted: false },
      { text: "(v2)", highlighted: true },
      { text: ".txt", highlighted: false },
    ]);
  });

  it("returns the whole name unhighlighted when no usable tokens", () => {
    expect(highlightName("readme.md", "* ?")).toEqual([
      { text: "readme.md", highlighted: false },
    ]);
  });

  it("returns [] for an empty name", () => {
    expect(highlightName("", "query")).toEqual([]);
  });
});

describe("nextResultLimit", () => {
  it("climbs the ladder and stops at the top", () => {
    expect(nextResultLimit(200)).toBe(500);
    expect(nextResultLimit(500)).toBe(1000);
    expect(nextResultLimit(1000)).toBe(2000);
    expect(nextResultLimit(2000)).toBe(null);
  });

  it("snaps an off-ladder value up to the next rung", () => {
    expect(nextResultLimit(100)).toBe(200);
    expect(nextResultLimit(5000)).toBe(null);
  });
});

describe("normalizeResultLimit", () => {
  it("keeps the configured cap within the safe backend range", () => {
    expect(normalizeResultLimit(undefined)).toBe(200);
    expect(normalizeResultLimit(12)).toBe(50);
    expect(normalizeResultLimit(750.9)).toBe(750);
    expect(normalizeResultLimit(20_001)).toBe(2_000);
  });
});
