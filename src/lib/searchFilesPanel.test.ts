import { describe, expect, test } from "bun:test";
import { getIndexDisplayError, mergeIndexedRoots, removeIndexedRoot } from "./searchFilesPanel";

describe("search files panel helpers", () => {
  test("mergeIndexedRoots appends new folders immediately and deduplicates", () => {
    expect(mergeIndexedRoots(["D:\\Docs"], ["D:\\Projects", "D:\\Docs"])).toEqual([
      "D:\\Docs",
      "D:\\Projects",
    ]);
  });

  test("removeIndexedRoot removes the clicked folder immediately", () => {
    expect(removeIndexedRoot(["D:\\Docs", "D:\\Projects"], "D:\\Docs")).toEqual(["D:\\Projects"]);
  });

  test("getIndexDisplayError hides per-file extraction failures", () => {
    expect(
      getIndexDisplayError(
        "Extraction error: failed parsing cross reference table: invalid start value",
      ),
    ).toBe(null);
  });

  test("getIndexDisplayError keeps real index failures visible", () => {
    expect(getIndexDisplayError("Index error: lock busy")).toBe("Index error: lock busy");
  });
});
