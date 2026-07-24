import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { Switch } from "./switch";

describe("Switch", () => {
  test("uses explicit off-state contrast tokens", () => {
    const html = renderToStaticMarkup(<Switch checked={false} />);

    expect(html).toContain("--switch-off-bg");
    expect(html).toContain("--switch-off-border");
    expect(html).toContain("--switch-off-thumb");
  });
});
