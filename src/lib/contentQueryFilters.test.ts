import { describe, expect, it, test } from "bun:test";
import { buildContentFilterTokens } from "./contentQueryFilters";
import { FILE_TYPE_EXTENSIONS } from "./fileNameSearch";
import type { SearchType } from "./fileNameSearch";

// Fixed instant for every date-based assertion below: 2026-07-09 local.
const NOW = new Date(2026, 6, 9);

describe("buildContentFilterTokens — type categories", () => {
  for (const [cat, exts] of Object.entries(FILE_TYPE_EXTENSIONS)) {
    test(`${cat} emits one comma-joined ext: token`, () => {
      const tokens = buildContentFilterTokens(new Set([cat as SearchType]), "any", "any", NOW);
      expect(tokens).toBe(`ext:${exts.join(",")}`);
    });
  }

  it("unions and comma-joins extensions across two selected categories", () => {
    const tokens = buildContentFilterTokens(new Set(["images", "documents"]), "any", "any", NOW);
    expect(tokens).toBe(
      `ext:${[...FILE_TYPE_EXTENSIONS.images, ...FILE_TYPE_EXTENSIONS.documents].join(",")}`,
    );
  });

  it("files-only produces no ext: token — content search has no folder concept", () => {
    expect(buildContentFilterTokens(new Set(["files"]), "any", "any", NOW)).toBe("");
  });

  it("folders-only produces no ext: token", () => {
    expect(buildContentFilterTokens(new Set(["folders"]), "any", "any", NOW)).toBe("");
  });

  it("files + folders + a real category only keeps the category's ext: token", () => {
    const tokens = buildContentFilterTokens(new Set(["files", "folders", "code"]), "any", "any", NOW);
    expect(tokens).toBe(`ext:${FILE_TYPE_EXTENSIONS.code.join(",")}`);
  });
});

describe("buildContentFilterTokens — size", () => {
  it("tiny", () => {
    expect(buildContentFilterTokens(new Set(), "tiny", "any", NOW)).toBe("size:<1mb");
  });

  it("medium emits a two-token range (backend requires two tokens for a range)", () => {
    expect(buildContentFilterTokens(new Set(), "medium", "any", NOW)).toBe("size:>=1mb size:<=100mb");
  });

  it("large", () => {
    expect(buildContentFilterTokens(new Set(), "large", "any", NOW)).toBe("size:>100mb");
  });

  it("huge", () => {
    expect(buildContentFilterTokens(new Set(), "huge", "any", NOW)).toBe("size:>1gb");
  });

  it("any emits nothing", () => {
    expect(buildContentFilterTokens(new Set(), "any", "any", NOW)).toBe("");
  });
});

describe("buildContentFilterTokens — date (absolute, computed from injected `now`)", () => {
  it("today → after:<now>", () => {
    expect(buildContentFilterTokens(new Set(), "any", "today", NOW)).toBe("after:2026-07-09");
  });

  it("week → after:<now - 7 days>", () => {
    expect(buildContentFilterTokens(new Set(), "any", "week", NOW)).toBe("after:2026-07-02");
  });

  it("month → after:<now - 1 calendar month>", () => {
    expect(buildContentFilterTokens(new Set(), "any", "month", NOW)).toBe("after:2026-06-09");
  });

  it("any emits nothing", () => {
    expect(buildContentFilterTokens(new Set(), "any", "any", NOW)).toBe("");
  });
});

describe("buildContentFilterTokens — combined / empty", () => {
  it("no chips active at all → empty string", () => {
    expect(buildContentFilterTokens(new Set(), "any", "any", NOW)).toBe("");
  });

  it("composes ext + size(range) + date tokens together, space-joined", () => {
    const tokens = buildContentFilterTokens(new Set(["images"]), "medium", "week", NOW);
    expect(tokens).toBe(
      `ext:${FILE_TYPE_EXTENSIONS.images.join(",")} size:>=1mb size:<=100mb after:2026-07-02`,
    );
  });

  it("defaults `now` to the current time when omitted", () => {
    // Just verify it doesn't throw and produces a well-formed after: token —
    // exact value is covered by the fixed-`now` cases above.
    const tokens = buildContentFilterTokens(new Set(), "any", "today");
    expect(tokens).toMatch(/^after:\d{4}-\d{2}-\d{2}$/);
  });
});
