import { describe, expect, test } from "bun:test";
import { getIndexDisplayError, getTabFilterSuggestion, mergeIndexedRoots, removeIndexedRoot } from "./searchFilesPanel";

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

  test("Tab recognizes a folder keyword as the visible Folders filter", () => {
    expect(getTabFilterSuggestion("project folder", new Set(), "any", "any")).toEqual({
      kind: "folders",
      label: "Folders",
      nextQuery: "project ",
    });
  });

  test("Tab does not offer a filter that is already active", () => {
    expect(getTabFilterSuggestion("folder", new Set(["folders"]), "any", "any")).toBeNull();
  });
});
