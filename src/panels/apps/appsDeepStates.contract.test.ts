import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const debloat = readFileSync("src/panels/apps/DebloatPanel.tsx", "utf8");
const packages = readFileSync("src/panels/apps/PackageUpdateTools.tsx", "utf8");

describe("apps populated-state interaction contracts", () => {
  test("debloat inventory items are keyboard-operable pressed buttons", () => {
    expect(debloat).toContain('className={`debloat-chip');
    expect(debloat).toContain("aria-pressed={selected}");
    expect(debloat).toContain("disabled={removing}");
    expect(debloat).not.toContain('className={`debloat-chip${selected ? " is-selected" : ""}`}\n      onClick={() => !removing');
  });

  test("package update selections expose their selected state", () => {
    expect(packages).toContain("aria-pressed={checked}");
    expect(packages).toContain('aria-label={`${checked ? "Deselect" : "Select"} ${title} update`}');
  });
});
