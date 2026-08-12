import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

describe("Sidebar mode controls", () => {
  test("groups interface and persona controls in one compact sidebar frame", () => {
    const sidebar = readFileSync("src/components/Sidebar.tsx", "utf8");

    const groupStart = sidebar.indexOf('className="preferences-row"');
    const groupEnd = sidebar.indexOf("{isDevBuild", groupStart);

    expect(groupStart).toBeGreaterThan(-1);
    expect(groupEnd).toBeGreaterThan(groupStart);

    const groupMarkup = sidebar.slice(groupStart, groupEnd);
    expect(groupMarkup).toContain("<ExperienceLevelSwitch compact />");
    expect(groupMarkup).toContain("<PersonaSwitch compact />");
  });
});
