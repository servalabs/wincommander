import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const debloat = readFileSync("src/panels/apps/DebloatPanel.tsx", "utf8");
const packages = readFileSync("src/panels/apps/PackageUpdateTools.tsx", "utf8");

describe("apps populated-state interaction contracts", () => {
  test("application review exposes native keyboard-operable selection and details", () => {
    const review = readFileSync("src/panels/apps/components/ApplicationReviewResults.tsx", "utf8");
    expect(debloat).toContain("<ApplicationReviewResults");
    expect(review).toContain('type="checkbox" checked={props.selected.has(item.id)}');
    expect(review).toContain("disabled={props.removing || !!item.removalUnavailable}");
    expect(review).toContain('aria-label={`Select ${item.label} for removal`}');
    expect(review).toContain('aria-expanded={detailId === item.id}');
  });

  test("package update selections expose their selected state", () => {
    expect(packages).toContain("aria-pressed={checked}");
    expect(packages).toContain('aria-label={`${checked ? "Deselect" : "Select"} ${packageName} update from ${manager}`}');
  });

});
