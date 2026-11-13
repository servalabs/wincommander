import { afterAll, expect, mock, test } from "bun:test";
import { JSDOM } from "jsdom";
import React, { act } from "react";
import { createRoot } from "react-dom/client";

const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://example.test" });
const previous = { window: globalThis.window, document: globalThis.document, act: globalThis.IS_REACT_ACT_ENVIRONMENT };
globalThis.window = dom.window;
globalThis.document = dom.window.document;
globalThis.IS_REACT_ACT_ENVIRONMENT = true;
afterAll(() => {
  mock.restore();
  dom.window.close();
  if (previous.window === undefined) delete globalThis.window; else globalThis.window = previous.window;
  if (previous.document === undefined) delete globalThis.document; else globalThis.document = previous.document;
  if (previous.act === undefined) delete globalThis.IS_REACT_ACT_ENVIRONMENT; else globalThis.IS_REACT_ACT_ENVIRONMENT = previous.act;
});

const scan = mock(async () => ({ scanId: "scan", groups: [{ id: "group", files: [
  { id: "keep", path: "keep", removable: true },
  { id: "remove", path: "remove", removable: true },
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

test("removal invalidates results and navigation does not trigger another scan", async () => {
  let root = createRoot(document.getElementById("root"));
  await act(async () => { root.render(React.createElement(Harness)); });
  expect(scan).toHaveBeenCalledTimes(1);
  expect(current.duplicateScan?.scanId).toBe("scan");
  await act(async () => { primeMaintenanceSessionValue("file-hygiene.empty-scan", { folders: [{ id: "prior" }] }); });
  await act(async () => { await current.remove("remove"); });
  expect(current.duplicateScan).toBeUndefined();
  expect(current.emptyScan).toBeUndefined();
  expect(scan).toHaveBeenCalledTimes(1);
  await act(async () => { root.unmount(); });
  root = createRoot(document.getElementById("root"));
  await act(async () => { root.render(React.createElement(Harness)); });
  expect(scan).toHaveBeenCalledTimes(1);
  await act(async () => { await current.scan("local"); });
  expect(scan).toHaveBeenCalledTimes(2);
  fail = true;
  await act(async () => { await current.remove("remove"); });
  expect(current.duplicateScan).toBeUndefined();
  expect(current.error).toContain("could not be confirmed");
  expect(history.at(-1).outcome).toBe("unknown");
  expect(scan).toHaveBeenCalledTimes(2);
  await act(async () => {
    primeMaintenanceSessionValue("file-hygiene.roots", ["fixture"]);
    current.changeTool("empty");
  });
  await act(async () => { await current.scan(); });
  expect(emptyScan).toHaveBeenCalledTimes(1);
  await act(async () => { primeMaintenanceSessionValue("file-hygiene.duplicate-scan", { scanId: "stale", groups: [] }); });
  await act(async () => { await current.remove("folder"); });
  expect(current.emptyScan).toBeUndefined();
  expect(current.duplicateScan).toBeUndefined();
  expect(emptyScan).toHaveBeenCalledTimes(1);
  await act(async () => { root.unmount(); });
});
