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
    expect(groupMarkup).not.toContain("sidebar-auto-fix-all");
    expect(groupMarkup).not.toContain("Automatically apply safe Fix All recommendations");
    expect(sidebar).toContain("hideSidebarPreferences === true");
    expect(sidebar).toContain('includes("sidebar-preferences")');
    expect(sidebar).toContain("{!preferencesHidden && (");
  });

  test("keeps the borrowed visibility controls as grouped three-state rows", () => {
    const table = readFileSync("src/panels/secret/VisibilityTable.tsx", "utf8");

    expect(table).toContain("Persona, interface &amp; licensing");
    expect(table).toContain("hideSidebarPreferences: vis === \"always\"");
    expect(table).toContain('"sidebar-preferences"');
    expect(table).toContain("Tour &amp; guide");
    expect(table).toContain("hideTour: vis === \"always\"");
  });

  test("defaults license and tour to hidden while borrowed and wires their visibility choices", () => {
    const defaults = readFileSync("src/lib/visibilityDefaults.ts", "utf8");
    const table = readFileSync("src/panels/secret/VisibilityTable.tsx", "utf8");
    const sidebar = readFileSync("src/components/Sidebar.tsx", "utf8");
    const titleBar = readFileSync("src/components/TitleBar.tsx", "utf8");
    const guide = readFileSync("src/components/guide/GuideHost.tsx", "utf8");

    expect(defaults).toContain('"license-panel"');
    expect(defaults).toContain('"tour"');
    expect(table).toContain('"license-panel"');
    expect(table).toContain('"tour"');
    expect(sidebar).toContain('includes("license-panel")');
    expect(titleBar).toContain('borrowedHidden.includes("tour")');
    expect(guide).toContain('borrowedHidden.includes("tour")');
  });
});
