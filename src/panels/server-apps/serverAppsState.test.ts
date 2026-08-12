import { describe, expect, test } from "bun:test";
import type { ServerAppConfig } from "../../types/settings";
import { resolveActiveServerAppId, resolveServerApps } from "./index";

const configured: ServerAppConfig[] = [
  { id: "console", name: "Console", url: "http://127.0.0.1:9000", icon: "console", customCss: "" },
];

describe("server apps state transitions", () => {
  test("uses defaults only before a setting exists", () => {
    expect(resolveServerApps(undefined).length).toBeGreaterThan(0);
    expect(resolveServerApps([])).toEqual([]);
    expect(resolveServerApps(configured)).toBe(configured);
  });

  test("repairs stale active tabs after settings load or an app is removed", () => {
    expect(resolveActiveServerAppId(configured, "gallery")).toBe("console");
    expect(resolveActiveServerAppId(configured, "console")).toBe("console");
    expect(resolveActiveServerAppId([], "console")).toBe("");
  });
});
