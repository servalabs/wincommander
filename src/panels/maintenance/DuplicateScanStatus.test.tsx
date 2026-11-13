import { expect, test } from "bun:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { DuplicateScanStatus } from "./DuplicateScanStatus";

test("duplicate scan explains its intentional discovery exclusions", () => {
  const html = renderToStaticMarkup(createElement(DuplicateScanStatus, { scanning: false, local: true }));
  expect(html).toContain("Discovery rules");
  expect(html).toContain("Dot-prefixed subfolders");
  expect(html).toContain("directly selected dot folder");
  expect(html).toContain(".bin, .dll and .jar");
  expect(html).toContain("narrower folder scans include them");
});
