import { expect, test } from "bun:test";
import { canRemoveDuplicateFile, getDuplicateSelectedBytes, getRetainedFileId, requiresSharedAdminConfirmation, selectDuplicateCopies } from "./maintenanceSelection";

test("shared admin confirmation applies only to selected copies", () => {
  const groups = [{ id: "g", files: [{ id: "shared", requiresSharedAdminConfirmation: true }, { id: "ordinary" }] }] as never;
  expect(requiresSharedAdminConfirmation(groups, new Set(["ordinary"]))).toBe(false);
  expect(requiresSharedAdminConfirmation(groups, new Set(["shared"]))).toBe(true);
  expect(requiresSharedAdminConfirmation(groups, new Set())).toBe(false);
});
import type { DuplicateGroup } from "../hooks/useBackend";

const group: DuplicateGroup = { id: "group", size: 100, reclaimableBytes: 200, fileCount: 3, memberOffset: 0, hasMoreFiles: false, files: [
  { id: "long", name: "a", path: "C:\\backups\\copy\\a", size: 100, modified: "100", removable: true },
  { id: "short", name: "a", path: "C:\\a", size: 100, modified: "300", removable: true },
  { id: "third", name: "a", path: "C:\\other\\a", size: 100, modified: "200", removable: true },
] };

test("smart selection keeps one copy and counts only removable bytes", () => {
  const selected = selectDuplicateCopies([group], "first");
  expect([...selected]).toEqual(["short", "third"]);
  expect(getDuplicateSelectedBytes([group], selected)).toBe(200);
});

test("shortest path preference retains the original with the shortest location", () => {
  expect(getRetainedFileId(group, "shortest-path")).toBe("short");
  expect(selectDuplicateCopies([group], "shortest-path").has("short")).toBe(false);
});

test("modified-date preferences retain the requested verified copy", () => {
  expect(getRetainedFileId(group, "newest-modified")).toBe("short");
  expect(getRetainedFileId(group, "oldest-modified")).toBe("long");
  expect([...selectDuplicateCopies([group], "newest-modified")]).toEqual(["long", "third"]);
});

test("unavailable modification dates never override a measured date", () => {
  const withMissingDate = { ...group, files: [{ ...group.files[0], modified: null }, ...group.files.slice(1)] };
  expect(getRetainedFileId(withMissingDate, "newest-modified")).toBe("short");
  expect(getRetainedFileId(withMissingDate, "oldest-modified")).toBe("third");
});

test("explicit keep choice overrides automatic preference and rejects stale IDs", () => {
  expect(getRetainedFileId(group, "first", { group: "third" })).toBe("third");
  expect(getRetainedFileId(group, "first", { group: "expired" })).toBe("long");
  expect(getDuplicateSelectedBytes([group, group], new Set(["short", "expired"]))).toBe(100);
});

test("single member and empty groups never select their last copy", () => {
  expect(selectDuplicateCopies([{ ...group, files: group.files.slice(0, 1) }, { ...group, id: "empty", files: [] }], "first").size).toBe(0);
});

test("smart selection never includes protected or unclassified copies", () => {
  const protectedGroup = { ...group, files: [group.files[0], { ...group.files[1], removable: false }, { ...group.files[2], removable: undefined }] };
  expect(selectDuplicateCopies([protectedGroup], "first").size).toBe(0);
});

test("individual removal rejects a keeper, protected copy, and stale id", () => {
  const mixed = { ...group, files: [group.files[0], group.files[1], { ...group.files[2], removable: false }] };
  expect(canRemoveDuplicateFile([mixed], "long", "first")).toBe(false);
  expect(canRemoveDuplicateFile([mixed], "short", "first")).toBe(true);
  expect(canRemoveDuplicateFile([mixed], "third", "first")).toBe(false);
  expect(canRemoveDuplicateFile([mixed], "stale", "first")).toBe(false);
  expect(canRemoveDuplicateFile([mixed], "short", "first", { group: "short" })).toBe(false);
});
