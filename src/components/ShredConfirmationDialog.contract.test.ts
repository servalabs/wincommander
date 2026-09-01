import { describe, expect, test } from "bun:test";
import { mergeShredTargets } from "./ShredConfirmationDialog";

describe("ShredConfirmationDialog target queue", () => {
  test("combines files and folders from separate selections", () => {
    expect(mergeShredTargets(
      [{ path: "C:\\Users\\Ada\\Desktop\\notes.txt", name: "notes.txt", isDir: false }],
      [{ path: "D:\\Archive", name: "Archive", isDir: true }],
    )).toEqual([
      { path: "C:\\Users\\Ada\\Desktop\\notes.txt", name: "notes.txt", isDir: false },
      { path: "D:\\Archive", name: "Archive", isDir: true },
    ]);
  });

  test("deduplicates Windows paths across picker selections", () => {
    expect(mergeShredTargets(
      [{ path: "D:\\Archive", name: "Archive", isDir: true }],
      [{ path: "d:/archive/", name: "archive", isDir: true }],
    )).toHaveLength(1);
  });
});
