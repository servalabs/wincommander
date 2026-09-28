import { describe, expect, test } from "bun:test";
import { createElement, type ComponentProps } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import IndexControls from "./IndexControls";

const defaults: ComponentProps<typeof IndexControls> = {
  expanded: false, onToggle: () => {}, indexStatus: null,
  indexDisplayError: null, foldersReindexing: false, managementError: null,
  privacyStatus: null, roots: ["C:\\Documents"], reindexing: false, rescanning: false,
  onReindex: () => {}, onRescan: () => {}, onAddFolders: () => {}, onRemoveFolder: () => {},
};
const render = (props: Partial<typeof defaults> = {}) =>
  renderToStaticMarkup(createElement(IndexControls, { ...defaults, ...props }));

describe("always available index controls", () => {
  test("existing folders keep an accessible settings button without a query or results", () => {
    const markup = render();
    expect(markup).toContain("Indexed folders");
    expect(markup).toContain('aria-expanded="false"');
    expect(markup).toContain('aria-controls="sfp-indexed-folders"');
    expect(markup).not.toContain('role="listbox"');
    expect(markup).not.toContain("Add folder to index");
  });

  test("opening settings exposes one add button and the configured folder", () => {
    const markup = render({ expanded: true });
    expect(markup).toContain('aria-expanded="true"');
    expect(markup).toContain('id="sfp-indexed-folders"');
    expect(markup).toContain("C:\\Documents");
    expect(markup.match(/title="Add folder to index"/g)?.length).toBe(1);
  });

  test("first-run setup opens automatically with a single add action", () => {
    const markup = render({ roots: [] });
    expect(markup).toContain('aria-expanded="true"');
    expect(markup).toContain("No folders indexed");
    expect(markup.match(/title="Add folder to index"/g)?.length).toBe(1);
  });

  test("folder updates prevent overlapping add, remove, rescan and rebuild requests", () => {
    const markup = render({ expanded: true, foldersReindexing: true });
    expect(markup.match(/disabled=""/g)?.length).toBe(4);
    expect(markup).toContain("updating folders");
  });

  test("management failures remain visible even with settings collapsed", () => {
    const markup = render({ managementError: "Could not add that folder. Try again." });
    expect(markup).toContain('role="alert"');
    expect(markup).toContain("Could not add that folder. Try again.");
  });
});
