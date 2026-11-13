import { mock } from "bun:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import React, { act } from "react";
import { createRoot } from "react-dom/client";

const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://example.test" });
globalThis.window = dom.window;
globalThis.document = dom.window.document;
globalThis.getComputedStyle = dom.window.getComputedStyle.bind(dom.window);
globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const file = (id, profileScope = "current", removable = true) => ({ id, name: `${id}.txt`, path: id, profileScope, removable, modified: null, size: 4 });
const scan = mock(async () => ({ scanId: "scan", groups: [{ id: "group", fileCount: 58, size: 4, reclaimableBytes: 228, memberOffset: 0, hasMoreFiles: false, files: [
  file("keep"), file("remove"), file("protected", "current", false), file("outside", "outside"),
  ...Array.from({ length: 54 }, (_, index) => file(`copy-${index}`)),
] }] }));
let fail = false;
const remove = mock(async () => {
  if (fail) throw new Error("lost response");
  return { filesRemoved: 1, bytesRecovered: 4, errors: [], cancelled: false };
});
const emptyScan = mock(async () => ({ folders: [{ id: "folder", path: "fixture" }] }));
const emptyRemove = mock(async () => ({ foldersRemoved: 1, errors: [], cancelled: false }));
const useBackendMock = () => ({
  duplicateFinderScanLocal: scan, duplicateFinderRemove: remove,
  duplicateFinderProgress: async () => ({}),
  emptyFolderCleanerScan: emptyScan, emptyFolderCleanerRemove: emptyRemove,
});
mock.module("../../hooks/useBackend", () => ({ useBackend: useBackendMock, default: useBackendMock, executeBackendCommand: async () => ({}) }));
const history = [];
mock.module("../../hooks/useMaintenanceHistory", () => ({ recordMaintenanceOperation: (entry) => history.push(entry) }));

const { useFileHygiene } = await import("./useFileHygiene");
const { DuplicateGroups } = await import("./DuplicateGroups");
const { primeMaintenanceSessionValue } = await import("./maintenanceSessionState");
let current;
const noFilter = () => {};
function Harness() { current = useFileHygiene(); return React.createElement(DuplicateGroups, { tools: current, onFilterChange: noFilter, onRemoveFile: noFilter }); }

await (async () => {
  let root = createRoot(document.getElementById("root"));
  await act(async () => { root.render(React.createElement(Harness)); });
  assert.equal(scan.mock.calls.length, 1);
  assert.equal(current.duplicateScan?.scanId, "scan");
  const button = [...document.querySelectorAll("button")].find((item) => item.textContent === "Select shown copies matching profile");
  assert.ok(button);
  await act(async () => { button.click(); });
  assert.equal(current.selected.has("keep"), false);
  assert.equal(current.selected.has("protected"), false);
  assert.equal(current.selected.has("outside"), false);
  assert.equal(current.selected.has("remove"), true);
  assert.equal(current.selected.has("copy-46"), true);
  assert.equal(current.selected.has("copy-47"), false);
  await act(async () => { primeMaintenanceSessionValue("file-hygiene.empty-scan", { folders: [{ id: "prior" }] }); });
  await act(async () => { await current.remove("remove"); });
  assert.equal(current.duplicateScan, undefined);
  assert.equal(current.emptyScan, undefined);
  assert.equal(scan.mock.calls.length, 1);
  await act(async () => { root.unmount(); });
  root = createRoot(document.getElementById("root"));
  await act(async () => { root.render(React.createElement(Harness)); });
  assert.equal(scan.mock.calls.length, 1);
  await act(async () => { await current.scan("local"); });
  assert.equal(scan.mock.calls.length, 2);
  fail = true;
  await act(async () => { await current.remove("remove"); });
  assert.equal(current.duplicateScan, undefined);
  assert.match(current.error, /could not be confirmed/);
  assert.equal(history.at(-1).outcome, "unknown");
  assert.equal(scan.mock.calls.length, 2);
  await act(async () => {
    primeMaintenanceSessionValue("file-hygiene.roots", ["fixture"]);
    current.changeTool("empty");
  });
  await act(async () => { await current.scan(); });
  assert.equal(emptyScan.mock.calls.length, 1);
  await act(async () => { primeMaintenanceSessionValue("file-hygiene.duplicate-scan", { scanId: "stale", groups: [] }); });
  await act(async () => { await current.remove("folder"); });
  assert.equal(current.emptyScan, undefined);
  assert.equal(current.duplicateScan, undefined);
  assert.equal(emptyScan.mock.calls.length, 1);
  await act(async () => { root.unmount(); });
})();
