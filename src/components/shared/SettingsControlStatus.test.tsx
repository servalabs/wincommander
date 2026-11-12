import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import SettingsControlStatus from "./SettingsControlStatus";
import { getControlLifecycle } from "@/lib/settingsControlLifecycle";

describe("SettingsControlStatus", () => {
  test("renders the shared state, reason, and Windows account identity", () => {
    const html = renderToStaticMarkup(
      <SettingsControlStatus lifecycle={getControlLifecycle({
        failureReason: "store-read-only",
        account: { name: "example", displayName: "Example" },
      })} />,
    );

    expect(html).toContain("Blocked: store-read-only");
    expect(html).toContain("Windows account: Example");
  });
});
