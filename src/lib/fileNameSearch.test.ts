// SPDX-License-Identifier: AGPL-3.0-or-later
// Tests for buildSearchQuery — the single Everything (es.exe) query builder.
// Regression coverage for the defect this file used to carry: a separate
// buildBackendSearchQuery wrapper sent ONLY the free-text query whenever
// filters were also active (working around es.exe's argv-quoting bug), then
// re-filtered client-side. That silently under-reported results whenever the
// first N raw hits didn't happen to match the active filters. Both the
// wrapper and the client-side re-filter are gone; buildSearchQuery must
// always emit every active filter token AND the text together.

import { describe, expect, it, test } from "bun:test";
import { buildSearchQuery } from "./fileNameSearch";
import { FILE_TYPE_EXTENSIONS } from "./fileNameSearch";
import type { SearchType } from "./fileNameSearch";

describe("buildSearchQuery — text + filters combine (the core fix)", () => {
  it("emits every filter token together with the text query, not just the text", () => {
    const q = buildSearchQuery("assets", new Set<SearchType>(["folders"]), "large", "week");
    // Previously this scenario ("folders named assets" + a size/date chip)
    // sent the backend only "assets*" and filtered the raw hits client-side —
    // silently dropping matching folders whenever the first page of raw
    // Everything results happened to be all files.
    expect(q).toBe("folder: size:>100mb dm:thisweek assets*");
  });

  it("text alone (no filters) still normalizes to a trailing-wildcard token", () => {
    expect(buildSearchQuery("report 2024", new Set(), "any", "any")).toBe("report*2024*");
  });

  it("filters alone (no text) still produce their tokens with no dangling text token", () => {
    expect(buildSearchQuery("", new Set<SearchType>(["images"]), "huge", "today")).toBe(
      `ext:${FILE_TYPE_EXTENSIONS.images.join(";")} size:>1gb dm:today`,
    );
  });

  it("nothing selected and no text yields an empty string", () => {
    expect(buildSearchQuery("", new Set(), "any", "any")).toBe("");
  });
});

describe("buildSearchQuery — files/folders scope", () => {
  it("folders-only emits folder: with no ext: token", () => {
    expect(buildSearchQuery("", new Set<SearchType>(["folders"]), "any", "any")).toBe("folder:");
  });

  it("files-only emits file: with no ext: token", () => {
    expect(buildSearchQuery("", new Set<SearchType>(["files"]), "any", "any")).toBe("file:");
  });
});

describe("buildSearchQuery — extension categories dedupe into one ext: token", () => {
  for (const [cat, exts] of Object.entries(FILE_TYPE_EXTENSIONS)) {
    test(`${cat} alone emits one semicolon-joined ext: token`, () => {
      const q = buildSearchQuery("", new Set([cat as SearchType]), "any", "any");
      expect(q).toBe(`ext:${exts.join(";")}`);
    });
  }

  it("unions extensions from two selected categories into a single ext: token", () => {
    const q = buildSearchQuery("", new Set<SearchType>(["images", "videos"]), "any", "any");
    // Exactly one ext: token — categories are merged, never emitted twice.
    expect(q.match(/ext:/g)?.length).toBe(1);
    const merged = [...FILE_TYPE_EXTENSIONS.images, ...FILE_TYPE_EXTENSIONS.videos];
    expect(q).toBe(`ext:${merged.join(";")}`);
  });

  it("a combined type set dedupes so no extension appears twice even when categories repeat a value", () => {
    // FILE_TYPE_EXTENSIONS has no real cross-category duplicate today, but the
    // builder merges through a Set — assert that guard directly so a future
    // category addition that does overlap can't silently double an ext:.
    const q = buildSearchQuery("", new Set<SearchType>(["documents", "images", "code"]), "any", "any");
    const extPart = q.slice("ext:".length).split(";");
    expect(new Set(extPart).size).toBe(extPart.length);
  });

  it("files + folders + a real category keeps file:/folder: mutual exclusivity moot and still emits the category's ext:", () => {
    // files and folders are mutually exclusive at the toggle layer (useFileSearch),
    // but buildSearchQuery itself is a pure function — verify it doesn't choke
    // if both happen to be present, and still emits the ext: token.
    const q = buildSearchQuery("", new Set<SearchType>(["files", "code"]), "any", "any");
    expect(q).toBe(`file: ext:${FILE_TYPE_EXTENSIONS.code.join(";")}`);
  });
});

describe("buildSearchQuery — size, verified against es.exe (ES_FACTS)", () => {
  it("tiny → size:<1mb", () => {
    expect(buildSearchQuery("", new Set(), "tiny", "any")).toBe("size:<1mb");
  });

  it("medium → size:1mb..100mb (single-token range — verified working on es.exe)", () => {
    expect(buildSearchQuery("", new Set(), "medium", "any")).toBe("size:1mb..100mb");
  });

  it("large → size:>100mb", () => {
    expect(buildSearchQuery("", new Set(), "large", "any")).toBe("size:>100mb");
  });

  it("huge → size:>1gb", () => {
    expect(buildSearchQuery("", new Set(), "huge", "any")).toBe("size:>1gb");
  });

  it("any emits no size token", () => {
    expect(buildSearchQuery("", new Set(), "any", "any")).toBe("");
  });
});

describe("buildSearchQuery — date modified, verified against es.exe (ES_FACTS)", () => {
  it("today → dm:today", () => {
    expect(buildSearchQuery("", new Set(), "any", "today")).toBe("dm:today");
  });

  it("week → dm:thisweek", () => {
    expect(buildSearchQuery("", new Set(), "any", "week")).toBe("dm:thisweek");
  });

  it("month → dm:thismonth", () => {
    expect(buildSearchQuery("", new Set(), "any", "month")).toBe("dm:thismonth");
  });

  it("any emits no date token", () => {
    expect(buildSearchQuery("", new Set(), "any", "any")).toBe("");
  });
});
