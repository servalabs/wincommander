import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { CompatButton } from "./compat-button";

describe("CompatButton", () => {
  test("preserves the eager legacy minimal/small button contract without bp", () => {
    const html = renderToStaticMarkup(
      <CompatButton minimal small icon="refresh">Refresh</CompatButton>,
    );

    expect(html).toContain("Refresh");
    expect(html).toContain("h-8");
    expect(html).toContain("bg-transparent");
  });
});
