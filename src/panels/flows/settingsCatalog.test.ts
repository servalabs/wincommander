import { describe, expect, test } from "bun:test";
import {
  buildFlowSettingOptions,
  formatFlowSettingValue,
  isFlowSettingPathSafe,
  parseFlowSettingValue,
} from "./settingsCatalog";

describe("flow settings catalog", () => {
  test("discovers hundreds of live setting leaves without exposing secret or flow payloads", () => {
    const safeSettings = Object.fromEntries(
      Array.from({ length: 240 }, (_, index) => [`setting${index}`, index % 2 === 0]),
    );
    const options = buildFlowSettingOptions({
      ideal: { matrix: safeSettings },
      app: {
        theme: "dark",
        startupPin: { realHash: "must-not-appear" },
        unlockKeyword: "must-not-appear",
        proFlows: [{ secret: "must-not-appear" }],
        contingency: { transportToken: "must-not-appear" },
        lastPanel: "flows",
      },
    });

    const discovered = options.filter((option) => option.path.startsWith("ideal.matrix."));
    expect(discovered).toHaveLength(240);
    expect(options.some((option) => option.path === "app.theme")).toBe(true);
    expect(options.some((option) => option.path.includes("realHash"))).toBe(false);
    expect(options.some((option) => option.path.includes("unlockKeyword"))).toBe(false);
    expect(options.some((option) => option.path.startsWith("app.proFlows"))).toBe(false);
    expect(options.some((option) => option.path.startsWith("app.contingency"))).toBe(false);
    expect(options.some((option) => option.path === "app.lastPanel")).toBe(false);
  });

  test("round-trips representative JSON values used by trigger and condition editors", () => {
    const values: unknown[] = [
      true,
      false,
      null,
      0,
      42,
      -12.5,
      "",
      "Allow",
      "text with spaces",
      ["one", "two"],
      { enabled: true, count: 3 },
    ];
    for (const value of values) {
      expect(parseFlowSettingValue(formatFlowSettingValue(value))).toEqual(value);
    }
  });

  test("keeps plain unquoted input as a string", () => {
    expect(parseFlowSettingValue("Allow")).toBe("Allow");
  });

  test("matches the backend secret and noise path policy", () => {
    for (const path of [
      "app.flowSigningSeedB64",
      "app.unlockKeyword",
      "ideal.privacy.startupPin.decoyHash",
      "app.fleet.enrollmentToken",
      "app.proFlows",
      "app.lastPanel",
    ]) {
      expect(isFlowSettingPathSafe(path)).toBe(false);
    }
    expect(isFlowSettingPathSafe("ideal.privacy.telemetry.windowsDisabled")).toBe(true);
    expect(isFlowSettingPathSafe("current.security.firewallEnabled")).toBe(true);
  });
});
