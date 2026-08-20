import { expect, test } from "bun:test";
declare const Bun: { file(path: string): { text(): Promise<string> } };

test("panel and Ctrl+Space overlay share typed diagnostics and preserve combobox semantics", async () => {
  const panel = await Bun.file("src/panels/search-files/index.tsx").text();
  const overlay = await Bun.file("src/components/EverythingSearchBar.tsx").text();
  for (const source of [panel, overlay]) expect(source).toContain('fileSearchDiagnostic');
  expect(panel).toContain('aria-activedescendant');
  expect(overlay).toContain('aria-activedescendant');
  expect(panel).toContain('aria-live="polite"');
});
