import { mock } from "bun:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import React, { act } from "react";
import { createRoot } from "react-dom/client";

const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://example.test" });
globalThis.window = dom.window;
globalThis.document = dom.window.document;
globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const scan = mock(async () => ({ scanId: "scan", groups: [{ id: "group", files: [
  { id: "keep", path: "keep", removable: true },
  { id: "remove", path: "remove", removable: true },
  { id: "protected", path: "protected", removable: false },
] }] }));
let fail = false;
const remove = mock(async () => {
  if (fail) throw new Error("lost response");
  return { filesRemoved: 1, bytesRecovered: 4, errors: [], cancelled: false };
});
const emptyScan = mock(async () => ({ folders: [{ id: "folder", path: "fixture" }] }));
const emptyRemove = mock(async () => ({ foldersRemoved: 1, errors: [], cancelled: false }));
mock.module("../../hooks/useBackend", () => ({ useBackend: () => ({
  duplicateFinderScanLocal: scan, duplicateFinderRemove: remove,
  duplicateFinderProgress: async () => ({}),
  emptyFolderCleanerScan: emptyScan, emptyFolderCleanerRemove: emptyRemove,
}) }));
const history = [];
mock.module("../../hooks/useMaintenanceHistory", () => ({ recordMaintenanceOperation: (entry) => history.push(entry) }));

const { useFileHygiene } = await import("./useFileHygiene");
const { primeMaintenanceSessionValue } = await import("./maintenanceSessionState");
let current;
function Harness() { current = useFileHygiene(); return null; }

await (async () => {
  let root = createRoot(document.getElementById("root"));
  await act(async () => { root.render(React.createElement(Harness)); });
  assert.equal(scan.mock.calls.length, 1);
  assert.equal(current.duplicateScan?.scanId, "scan");
  await act(async () => { current.selectGroupCopies("group"); });
  assert.deepEqual([...current.selected], ["remove"]);
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
