import { expect, test } from "bun:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { DuplicateGroups } from "./DuplicateGroups";
import { canRemoveDuplicateFile, getRetainedFileId, selectDuplicateCopies } from "../../lib/maintenanceSelection";

test("Git object copies are hidden behind a disclosure and excluded from selection even with stale eligibility", () => {
  const git = { id: "git", name: "ca958bda29ff75dda1e59050be45efaa85974ed9d1f8c3ca1c9da45273f753c7", path: "D:\\Repo\\.git\\lfs\\objects\\ca\\95\\object", size: 10, modified: null, removable: true };
  const keeper = { id: "keeper", name: "Theron_Signals.mp4", path: "D:\\Videos\\Theron_Signals.mp4", size: 10, modified: null, removable: true };
  const group = { id: "g", files: [git, keeper], fileCount: 2, size: 10, reclaimableBytes: 10, memberOffset: 0, hasMoreFiles: false };
  const tools = { duplicateScan: { groups: [group] }, selected: new Set(), busy: false, retainedFileId: () => "keeper", select() {}, openDuplicate() {}, keepFile() {}, loadMembers() {} };
  const html = renderToStaticMarkup(createElement(DuplicateGroups, { tools: tools as never, onFilterChange() {}, onRemoveFile() {} }));
  expect(html).toContain("Theron_Signals.mp4");
  expect(html).toContain("full-content hash matches");
  expect(html).toContain("Show 1 Git-managed copy on this page");
  expect(html).not.toContain(git.name);
  expect(getRetainedFileId(group as never, "first")).toBe("keeper");
  expect(getRetainedFileId(group as never, "first", { g: "git" })).toBe("keeper");
  expect(canRemoveDuplicateFile([group] as never, "git", "first", { g: "keeper" })).toBe(false);
  expect(selectDuplicateCopies([group] as never, "first", { g: "keeper" }).has("git")).toBe(false);
});

test("protected copies are visible for review and cannot be removed", () => {
  const protectedFile = { id: "keeper", name: "protected-app.exe", path: "C:\\other\\protected-app.exe", size: 10, modified: null, removable: false, profileScope: "other", removalBlockReason: "other_profile" };
  const eligible = { id: "copy", name: "photo.jpg", path: "C:\\Users\\me\\photo.jpg", size: 10, modified: null, removable: true, profileScope: "current" };
  const group = { id: "g", files: [protectedFile, eligible], fileCount: 2, size: 10, reclaimableBytes: 10, memberOffset: 0, hasMoreFiles: false };
  const tools = { duplicateScan: { groups: [group] }, selected: new Set(), busy: false, retainedFileId: () => "keeper", select() {}, openDuplicate() {}, keepFile() {}, loadMembers() {} };
  const html = renderToStaticMarkup(createElement(DuplicateGroups, { tools: tools as never, onFilterChange() {}, onRemoveFile() {} }));
  expect(html).toContain("protected-app.exe");
  expect(html).toContain("Kept · review only");
  expect(html).toContain("Another user&#x27;s files are protected");
  expect(html).not.toContain("Remove protected-app.exe");
  expect(html).toContain("Remove photo.jpg");
  expect(html).toContain("including protected");
  expect(html).toContain('aria-label="Filter duplicate file type"');
  expect(html).toContain('aria-pressed="true"');
  expect(html).toContain("Select copies");
  expect(html).toContain('title="Remove file"');
});

test("older results without a positive removal classification are not exposed as actionable", () => {
  const file = { id: "unknown", name: "unverified.dat", path: "C:\\Users\\me\\unverified.dat", size: 10, modified: null };
  const group = { id: "g", files: [file], fileCount: 2, size: 10, reclaimableBytes: 10, memberOffset: 0, hasMoreFiles: false };
  const tools = { duplicateScan: { groups: [group] }, selected: new Set(), busy: false, retainedFileId: () => "unknown", select() {}, openDuplicate() {}, keepFile() {}, loadMembers() {} };
  const html = renderToStaticMarkup(createElement(DuplicateGroups, { tools: tools as never, onFilterChange() {}, onRemoveFile() {} }));
  expect(html).toContain("unverified.dat");
  expect(html).toContain("review only");
  expect(html).not.toContain("Remove unverified.dat");
});

test("a protected copy that is not the keeper stays review-only", () => {
  const protectedFile = { id: "other", name: "other-user.txt", path: "C:\\Users\\Other\\other-user.txt", size: 10, modified: null, removable: false, profileScope: "other", removalBlockReason: "other_profile" };
  const keeper = { id: "keeper", name: "my-copy.txt", path: "C:\\Users\\Me\\my-copy.txt", size: 10, modified: null, removable: true, profileScope: "current" };
  const group = { id: "g", files: [keeper, protectedFile], fileCount: 2, size: 10, reclaimableBytes: 10, memberOffset: 0, hasMoreFiles: false };
  const tools = { duplicateScan: { groups: [group] }, selected: new Set(), busy: false, retainedFileId: () => "keeper", select() {}, openDuplicate() {}, keepFile() {}, loadMembers() {} };
  const html = renderToStaticMarkup(createElement(DuplicateGroups, { tools: tools as never, onFilterChange() {}, onRemoveFile() {} }));
  expect(html).toContain("other-user.txt");
  expect(html).toContain("Review only");
  expect(html).not.toContain("Remove other-user.txt");
  expect(html).not.toContain("Keep other-user.txt");
  expect(html).toContain("Another user&#x27;s files are protected");
});

test("eligible files in an explicitly scanned location outside a profile are visible by default", () => {
  const file = { id: "copy", name: "report.txt", path: "D:\\Work\\report.txt", size: 10, modified: null, removable: true, profileScope: "outside" };
  const group = { id: "g", files: [file], fileCount: 2, size: 10, reclaimableBytes: 10, memberOffset: 0, hasMoreFiles: false };
  const tools = { duplicateScan: { groups: [group] }, selected: new Set(), busy: false, retainedFileId: () => "other", select() {}, openDuplicate() {}, keepFile() {}, loadMembers() {} };
  const html = renderToStaticMarkup(createElement(DuplicateGroups, { tools: tools as never, onFilterChange() {}, onRemoveFile() {} }));
  expect(html).toContain("report.txt");
  expect(html).toContain("All scanned locations");
});

test("shared administrator copies show a confirmation warning and remain actionable", () => {
  const file = { id: "copy", name: "report.txt", path: "D:\\Work\\report.txt", size: 10, modified: null, removable: true, requiresSharedAdminConfirmation: true };
  const group = { id: "g", files: [file], fileCount: 2, size: 10, reclaimableBytes: 10, memberOffset: 0, hasMoreFiles: false };
  const tools = { duplicateScan: { groups: [group] }, selected: new Set(), busy: false, retainedFileId: () => "other", select() {}, openDuplicate() {}, keepFile() {}, loadMembers() {} };
  const html = renderToStaticMarkup(createElement(DuplicateGroups, { tools: tools as never, onFilterChange() {}, onRemoveFile() {} }));
  expect(html).toContain("Shared Administrators ownership");
  expect(html).toContain("Remove report.txt");
  expect(html).toContain("Open containing folder");
});

test("unknown owner reason is visible without a hover", () => {
  const file = { id: "x", name: "x.txt", path: "D:\\Work\\x.txt", size: 10, modified: null, removable: false, removalBlockReason: "owner_unavailable" };
  const group = { id: "g", files: [file], fileCount: 2, size: 10, reclaimableBytes: 10, memberOffset: 0, hasMoreFiles: false };
  const tools = { duplicateScan: { groups: [group] }, selected: new Set(), busy: false, retainedFileId: () => "other", select() {}, openDuplicate() {}, keepFile() {}, loadMembers() {} };
  const html = renderToStaticMarkup(createElement(DuplicateGroups, { tools: tools as never, onFilterChange() {}, onRemoveFile() {} }));
  expect(html).toContain('class="duplicate-member-reason">Windows could not verify the owner');
  expect(html).toContain("Open containing folder");
});

test("a page of protected copies still offers access to later copies", () => {
  const file = { id: "keeper", name: "protected.exe", path: "C:\\other\\protected.exe", size: 10, modified: null, removable: false };
  const group = { id: "g", files: [file], fileCount: 201, size: 10, reclaimableBytes: 2000, memberOffset: 0, hasMoreFiles: true };
  const tools = { duplicateScan: { groups: [group] }, selected: new Set(), busy: false, retainedFileId: () => "keeper", select() {}, openDuplicate() {}, keepFile() {}, loadMembers() {} };
  const html = renderToStaticMarkup(createElement(DuplicateGroups, { tools: tools as never, onFilterChange() {}, onRemoveFile() {} }));
  expect(html).toContain("protected.exe");
  expect(html).not.toContain("Remove protected.exe");
  expect(html).toContain("Next copies");
});
