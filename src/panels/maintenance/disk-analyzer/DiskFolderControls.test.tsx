import { expect, test } from "bun:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import DiskItemDetails from "./DiskItemDetails";
import DiskItemList from "./DiskItemList";
import type { DiskNode } from "@/lib/diskAnalyzer";

const folder: DiskNode = { entryId: 4, name: "Test folder", fullPath: "C:\\Test folder", size: 100, allocated: null, isDir: true, lastModified: "", fileCount: 1, folderCount: 0 };
const noop = () => {};

test("folder deletion controls remain disabled without backend availability", () => {
  const list = renderToStaticMarkup(createElement(DiskItemList, { items: [folder], total: 100, selected: new Set<string>(), focused: null, disabled: false, folderDeleteAvailable: false, onFocus: noop, onSelect: noop, onNavigate: noop }));
  const details = renderToStaticMarkup(createElement(DiskItemDetails, { item: folder, disabled: false, folderDeleteAvailable: false, onOpen: noop, onNavigate: noop, onDelete: noop }));
  expect(list).toContain("Folder deletion unavailable");
  expect(list).toMatch(/disabled=""/);
  expect(details).toContain("Folder deletion is unavailable in this build.");
  expect(details).toMatch(/disabled=""[^>]*aria-label="Review deletion of Test folder"/);
});

test("available folder deletion requires separate native confirmation", () => {
  const list = renderToStaticMarkup(createElement(DiskItemList, { items: [folder], total: 100, selected: new Set<string>(), focused: null, disabled: false, folderDeleteAvailable: true, onFocus: noop, onSelect: noop, onNavigate: noop }));
  const details = renderToStaticMarkup(createElement(DiskItemDetails, { item: folder, disabled: false, folderDeleteAvailable: true, onOpen: noop, onNavigate: noop, onDelete: noop }));
  expect(list).not.toContain("Folder deletion unavailable");
  expect(details).toContain("Folder deletion requires a separate Windows confirmation.");
  expect(details).toContain("Review deletion of Test folder");
});
