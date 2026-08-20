import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { CheckboxControl } from "./bp";

test("themed checkbox renders a filled checked state with a tick", () => {
  const html = renderToStaticMarkup(
    <CheckboxControl checked ariaLabel="Clipboard action" />,
  );

  expect(html).toContain('data-state="checked"');
  expect(html).toContain("data-[state=checked]:bg-[var(--accent)]");
  expect(html).toContain("stroke=");
});
