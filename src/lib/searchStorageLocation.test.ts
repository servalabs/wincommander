// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from "bun:test";
import { parseKnownFolderScope, parseSearchStorageLocation, recentSearchFolders } from "./searchStorageLocation";
import { addChip, demoteLastChip } from "./searchTokens";

describe("parseSearchStorageLocation", () => {
  it("recognises drive tokens in either supported order", () => {
    expect(parseSearchStorageLocation("D:")).toEqual({ path: "D:\\", source: "D:" });
    expect(parseSearchStorageLocation(":d")).toEqual({ path: "D:\\", source: ":d" });
  });

  it("recognises full Windows paths and normalises slash direction", () => {
    expect(parseSearchStorageLocation("d:\\Projects\\WinCommander")).toEqual({
      path: "D:\\Projects\\WinCommander",
      source: "d:\\Projects\\WinCommander",
    });
    expect(parseSearchStorageLocation("E:/Documents/Reports")).toEqual({
      path: "E:\\Documents\\Reports",
      source: "E:/Documents/Reports",
    });
    expect(parseSearchStorageLocation("D:\\Project Files\\WinCommander")).toEqual({
      path: "D:\\Project Files\\WinCommander",
      source: "D:\\Project Files\\WinCommander",
    });
  });

  it("does not steal ordinary searches or a location embedded in one", () => {
    expect(parseSearchStorageLocation("report")).toBeNull();
    expect(parseSearchStorageLocation("report D:")).toBeNull();
    expect(parseSearchStorageLocation("D")).toBeNull();
  });

  it("keeps the original location in the query when its storage chip is undone", () => {
    const location = parseSearchStorageLocation("D:\\Project Files")!;
    const state = addChip({ chips: [], text: "" }, "in", {
      path: location.path,
      pathLabel: location.path,
      source: location.source,
    });
    expect(demoteLastChip(state)).toEqual({ chips: [], text: "D:\\Project Files" });
  });

  it("recognises Downloads after in or on and keeps the exact wording for undo", () => {
    const folders = [{ label: "Downloads", path: "D:\\Personal\\Downloads" }];
    expect(parseKnownFolderScope("invoice in Downloads", folders)).toEqual({
      query: "invoice",
      source: "in Downloads",
      folder: folders[0],
    });
    expect(parseKnownFolderScope("on downloads", folders)).toEqual({
      query: "",
      source: "on downloads",
      folder: folders[0],
    });
    expect(parseKnownFolderScope("part on download", folders)).toEqual({
      query: "part",
      source: "on download",
      folder: folders[0],
    });
  });

  it("undoes a typed storage scope without restoring deleted query text", () => {
    const folders = [{ label: "Downloads", path: "D:\\Personal\\Downloads" }];
    const scope = parseKnownFolderScope("part on download", folders)!;
    const state = addChip({ chips: [], text: scope.query }, "in", {
      path: scope.folder.path,
      pathLabel: scope.folder.label,
      source: scope.source,
    });
    expect(demoteLastChip(state)).toEqual({ chips: [], text: "on download" });
  });

  it("recognises a full folder path after in or on", () => {
    expect(parseKnownFolderScope("part on D:\\Work\\Incoming", [])).toEqual({
      query: "part",
      source: "on D:\\Work\\Incoming",
      folder: { label: "Incoming", path: "D:\\Work\\Incoming" },
    });
  });

  it("offers parent folders from recently opened paths", () => {
    expect(recentSearchFolders([
      "D:\\Work\\Incoming\\part.txt",
      "D:\\Work\\Incoming\\other.txt",
      "C:\\Users\\Admin\\Downloads\\",
    ])).toEqual([
      { label: "Incoming", path: "D:\\Work\\Incoming" },
      { label: "Downloads", path: "C:\\Users\\Admin\\Downloads" },
    ]);
  });
});
