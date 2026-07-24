import { describe, expect, it } from "bun:test";
import type { ContentHit, IndexStatus, ContentQueryArgs, MatchKind } from "./wincmd-search";

describe("wincmd-search types", () => {
  it("ContentHit shape is assignable", () => {
    const hit: ContentHit = {
      doc_id:     "1",
      path:       "C:\\Users\\test\\file.txt",
      name:       "file.txt",
      ext:        "txt",
      mtime:      1700000000,
      size:       1024,
      score:      0.95,
      match_kind: "Keyword",
      snippet:    "hello <mark>world</mark>",
      author:     "",
      doc_title:  "",
      tags:       "",
    };
    expect(hit.doc_id).toBe("1");
    expect(hit.match_kind).toBe("Keyword");
  });

  it("IndexStatus defaults are representable", () => {
    const status: IndexStatus = {
      indexed_docs:     0,
      pending_docs:     0,
      is_indexing:      false,
      last_error:       null,
      index_size_bytes: 0,
    };
    expect(status.is_indexing).toBe(false);
  });

  it("ContentQueryArgs allows partial optional fields", () => {
    const args: ContentQueryArgs = { terms: "hello world" };
    expect(args.terms).toBe("hello world");
    expect(args.limit).toBeUndefined();
  });

  it("MatchKind union covers all variants", () => {
    const variants: MatchKind[] = ["Keyword", "Semantic", "Hybrid"];
    expect(variants).toHaveLength(3);
  });
});
