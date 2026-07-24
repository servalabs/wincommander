import { describe, expect, it } from "bun:test";
import {
  buildContentQueryArgs,
  chunksToText,
  contentHitToDisplayRow,
  dedupeContentRows,
  formatContentModified,
  formatSnippetWithHighlights,
  isNameOnlyMatch,
} from "./contentSearch";
import type { Chunk, ContentHit } from "../types/wincmd-search";

const MOCK_HIT: ContentHit = {
  doc_id:     "42",
  path:       "C:\\docs\\report.pdf",
  name:       "report.pdf",
  ext:        "pdf",
  mtime:      1700000000,
  size:       204800,
  score:      0.87,
  match_kind: "Keyword",
  snippet:    "The annual <mark>budget</mark> report covers Q4.",
  author:     "Jane Quill",
  doc_title:  "FY24 Budget Report",
  tags:       "finance quarterly",
};

describe("buildContentQueryArgs", () => {
  it("sets defaults", () => {
    const args = buildContentQueryArgs("hello");
    expect(args.limit).toBe(50);
    expect(args.offset).toBe(0);
    expect(args.keyword_only).toBe(true);
  });

  it("passes through overrides", () => {
    const args = buildContentQueryArgs("hello", 20, 10, false);
    expect(args.limit).toBe(20);
    expect(args.offset).toBe(10);
    expect(args.keyword_only).toBe(false);
  });
});

describe("formatSnippetWithHighlights", () => {
  it("parses marks into highlighted segments", () => {
    const segs = formatSnippetWithHighlights("The annual <mark>budget</mark> report.");
    expect(segs).toHaveLength(3);
    expect(segs[1].text).toBe("budget");
    expect(segs[1].highlighted).toBe(true);
  });

  it("returns single plain segment when no marks", () => {
    const segs = formatSnippetWithHighlights("plain text here");
    expect(segs).toHaveLength(1);
    expect(segs[0].highlighted).toBe(false);
  });

  it("decodes HTML entities so code snippets render literally", () => {
    const segs = formatSnippetWithHighlights('uses Vec&lt;T&gt; and a &amp;&amp; b and &quot;x&quot;');
    const text = segs.map((s) => s.text).join("");
    expect(text).toContain("Vec<T>");
    expect(text).toContain("&& b");
    expect(text).toContain('"x"');
    expect(text).not.toContain("&lt;");
    expect(text).not.toContain("&amp;");
  });

  it("decodes entities inside a highlight", () => {
    const segs = formatSnippetWithHighlights("a <mark>Vec&lt;T&gt;</mark> b");
    expect(segs[1].highlighted).toBe(true);
    expect(segs[1].text).toBe("Vec<T>");
  });
});

describe("formatContentModified", () => {
  it("returns — for epoch zero", () => {
    expect(formatContentModified(0)).toBe("—");
  });

  it("returns a non-empty string for a real timestamp", () => {
    const s = formatContentModified(1700000000);
    expect(s.length).toBeGreaterThan(0);
    expect(s).not.toBe("—");
  });
});

describe("chunksToText", () => {
  const mk = (field: "Title" | "Body", ordinal: number, text: string): Chunk => ({
    doc_id: "1", field, ordinal, text,
  });

  it("joins body chunks in order and drops the title chunk", () => {
    const out = chunksToText([mk("Title", 0, "My Doc"), mk("Body", 1, "hello "), mk("Body", 2, "world")]);
    expect(out).toBe("hello world");
  });

  it("falls back to all text when there are no body chunks", () => {
    const out = chunksToText([mk("Title", 0, "just a title")]);
    expect(out).toBe("just a title");
  });

  it("returns empty string for no chunks", () => {
    expect(chunksToText([])).toBe("");
  });
});

describe("contentHitToDisplayRow", () => {
  it("maps all fields", () => {
    const row = contentHitToDisplayRow(MOCK_HIT);
    expect(row.docId).toBe("42");
    expect(row.name).toBe("report.pdf");
    expect(row.snippetSegs[1].text).toBe("budget");
    expect(row.sizeDisplay).toMatch(/KB/);
    expect(row.author).toBe("Jane Quill");
    expect(row.docTitle).toBe("FY24 Budget Report");
    expect(row.tags).toBe("finance quarterly");
  });
});

describe("dedupeContentRows", () => {
  const row = (path: string) => contentHitToDisplayRow({ ...MOCK_HIT, path });

  it("drops rows whose path already appears in filename results (case-insensitive)", () => {
    const rows = [row("C:\\docs\\A.pdf"), row("C:\\docs\\b.pdf")];
    const out = dedupeContentRows(rows, ["c:\\docs\\a.PDF"]);
    expect(out.map((r) => r.path)).toEqual(["C:\\docs\\b.pdf"]);
  });

  it("returns rows untouched when there are no filename results", () => {
    const rows = [row("C:\\docs\\a.pdf")];
    expect(dedupeContentRows(rows, [])).toEqual(rows);
  });

  it("returns empty when every row is a duplicate", () => {
    const rows = [row("C:\\x\\y.txt")];
    expect(dedupeContentRows(rows, ["C:\\x\\y.txt"])).toEqual([]);
  });
});

describe("isNameOnlyMatch", () => {
  it("is true only for NameSubstring hits", () => {
    expect(isNameOnlyMatch({ matchKind: "NameSubstring" })).toBe(true);
    expect(isNameOnlyMatch({ matchKind: "Keyword" })).toBe(false);
    expect(isNameOnlyMatch({ matchKind: "Semantic" })).toBe(false);
    expect(isNameOnlyMatch({ matchKind: "Hybrid" })).toBe(false);
  });

  it("NameSubstring hits map through contentHitToDisplayRow", () => {
    const row = contentHitToDisplayRow({ ...MOCK_HIT, match_kind: "NameSubstring" });
    expect(isNameOnlyMatch(row)).toBe(true);
  });
});
