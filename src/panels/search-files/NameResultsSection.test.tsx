import { describe, expect, test } from "bun:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import NameResultsSection from "./NameResultsSection";

const results = Array.from({ length: 2_000 }, (_, index) => ({
  name: `report-${index}.txt`,
  directory: "C:\\reports",
  full_path: `C:\\reports\\report-${index}.txt`,
  size: "1024",
  modified: "2026-08-20",
}));

describe("NameResultsSection", () => {
  test("mounts a bounded row window and retains an off-screen keyboard selection", () => {
    const markup = renderToStaticMarkup(createElement(NameResultsSection, {
      results,
      query: "report",
      isSearching: false,
      showNoMatches: false,
      engineMissing: false,
      selectedIndex: 1_750,
      onSelect: () => {},
      onOpenFile: () => {},
      onOpenFolder: () => {},
      onCopyPath: () => {},
      copiedPath: null,
      scrollContainer: null,
      loadNativeIcons: false,
    }));

    expect((markup.match(/role="option"/g) ?? []).length < 49).toBe(true);
    expect(markup).toContain('id="sfp-opt-1750"');
    expect(markup).toContain('aria-selected="true"');
  });
});
