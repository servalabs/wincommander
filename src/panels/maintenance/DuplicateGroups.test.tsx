import { expect, test } from "bun:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { DuplicateGroups } from "./DuplicateGroups";

test("protected copies stay hidden while an eligible copy can be reviewed for removal", () => {
  const protectedFile = { id: "keeper", name: "protected-app.exe", path: "C:\\other\\protected-app.exe", size: 10, modified: null, removable: false, profileScope: "other" };
  const eligible = { id: "copy", name: "photo.jpg", path: "C:\\Users\\me\\photo.jpg", size: 10, modified: null, removable: true, profileScope: "current" };
  const group = { id: "g", files: [protectedFile, eligible], fileCount: 2, size: 10, reclaimableBytes: 10, memberOffset: 0, hasMoreFiles: false };
  const tools = { duplicateScan: { groups: [group] }, selected: new Set(), busy: false, retainedFileId: () => "keeper", select() {}, openDuplicate() {}, keepFile() {}, loadMembers() {} };
  const html = renderToStaticMarkup(createElement(DuplicateGroups, { tools: tools as never, onFilterChange() {}, onRemoveFile() {} }));
  expect(html).not.toContain("protected-app.exe");
  expect(html).toContain("The kept copy is protected and hidden");
  expect(html).toContain("Remove photo.jpg");
  expect(html).toContain("including protected");
  expect(html).toContain('aria-label="Filter duplicate file type"');
  expect(html).toContain('aria-pressed="true"');
  expect(html).toContain("Select shown copies matching profile");
  expect(html).toContain('title="Remove file"');
});

test("older results without a positive removal classification are not exposed as actionable", () => {
  const file = { id: "unknown", name: "unverified.dat", path: "C:\\Users\\me\\unverified.dat", size: 10, modified: null };
  const group = { id: "g", files: [file], fileCount: 2, size: 10, reclaimableBytes: 10, memberOffset: 0, hasMoreFiles: false };
  const tools = { duplicateScan: { groups: [group] }, selected: new Set(), busy: false, retainedFileId: () => "unknown", select() {}, openDuplicate() {}, keepFile() {}, loadMembers() {} };
  const html = renderToStaticMarkup(createElement(DuplicateGroups, { tools: tools as never, onFilterChange() {}, onRemoveFile() {} }));
  expect(html).not.toContain("unverified.dat");
  expect(html).not.toContain("Remove unverified.dat");
});

test("outside-profile-only groups require an explicit location filter", () => {
  const file = { id: "copy", name: "report.txt", path: "D:\\Work\\report.txt", size: 10, modified: null, removable: true, profileScope: "outside" };
  const group = { id: "g", files: [file], fileCount: 2, size: 10, reclaimableBytes: 10, memberOffset: 0, hasMoreFiles: false };
  const tools = { duplicateScan: { groups: [group] }, selected: new Set(), busy: false, retainedFileId: () => "other", select() {}, openDuplicate() {}, keepFile() {}, loadMembers() {} };
  const html = renderToStaticMarkup(createElement(DuplicateGroups, { tools: tools as never, onFilterChange() {}, onRemoveFile() {} }));
  expect(html).not.toContain("report.txt");
  expect(html).toContain("All scanned locations");
});

test("a page of protected copies still offers access to later copies", () => {
  const file = { id: "keeper", name: "protected.exe", path: "C:\\other\\protected.exe", size: 10, modified: null, removable: false };
  const group = { id: "g", files: [file], fileCount: 201, size: 10, reclaimableBytes: 2000, memberOffset: 0, hasMoreFiles: true };
  const tools = { duplicateScan: { groups: [group] }, selected: new Set(), busy: false, retainedFileId: () => "keeper", select() {}, openDuplicate() {}, keepFile() {}, loadMembers() {} };
  const html = renderToStaticMarkup(createElement(DuplicateGroups, { tools: tools as never, onFilterChange() {}, onRemoveFile() {} }));
  expect(html).not.toContain("protected.exe");
  expect(html).toContain("Next copies");
});
