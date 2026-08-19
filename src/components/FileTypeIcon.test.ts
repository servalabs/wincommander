import { describe, expect, test } from "bun:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import FileTypeIcon, { FILE_TYPE_ICON_KINDS } from "./FileTypeIcon";

const OFFICIAL_PNG_KINDS = new Set(["pdf", "word", "excel", "slides"]);

describe("FileTypeIcon", () => {
  test("renders a unique icon for every type kind", () => {
    const markup = FILE_TYPE_ICON_KINDS.map((kind) => {
      const html = renderToStaticMarkup(createElement(FileTypeIcon, { kind }));
      expect(html).toContain(`data-kind="${kind}"`);
      if (OFFICIAL_PNG_KINDS.has(kind)) {
        expect(html).toContain("<img");
      } else {
        expect(html).toContain("<svg");
      }
      return html;
    });
    expect(new Set(markup).size).toBe(FILE_TYPE_ICON_KINDS.length);
  });

  test("unknown kinds fall back to a generic document svg", () => {
    const html = renderToStaticMarkup(createElement(FileTypeIcon, { kind: "today" }));
    expect(html).toContain("<svg");
    expect(html).toContain('data-kind="today"');
  });
});
