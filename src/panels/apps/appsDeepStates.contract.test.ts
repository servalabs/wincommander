import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const debloat = readFileSync("src/panels/apps/DebloatPanel.tsx", "utf8");
const packages = readFileSync("src/panels/apps/PackageUpdateTools.tsx", "utf8");
const appsPanel = readFileSync("src/panels/apps/index.tsx", "utf8");
const appsStyles = readFileSync("src/panels/apps/index.css", "utf8");

describe("apps populated-state interaction contracts", () => {
  test("application review exposes native keyboard-operable selection and details", () => {
    const review = readFileSync("src/panels/apps/components/ApplicationReviewResults.tsx", "utf8");
    expect(debloat).toContain("<ApplicationReviewResults");
    expect(review).toContain('type="checkbox" checked={props.selected.has(item.id)}');
    expect(review).toContain("disabled={props.removing || !!item.removalUnavailable}");
    expect(review).toContain('aria-label={`Select ${item.label} for removal`}');
    expect(review).toContain('aria-expanded={detailId === item.id}');
    expect(review).toContain('<dt>Reported size</dt>');
    expect(review).not.toContain('actual space freed may differ');
    expect(review).toContain('toExplorerDisplayPath(detail?.installLocation)');
  });

  test("package update selections expose their selected state", () => {
    expect(packages).toContain("aria-pressed={checked}");
    expect(packages).toContain('aria-label={`${checked ? "Deselect" : "Select"} ${packageName} update from ${manager}`}');
  });

  test("debloat keeps long app lists and details inside its review workspace", () => {
    expect(appsPanel).toContain('apps-panel--debloat');
    expect(appsStyles).toContain('.apps-panel--debloat .application-review-results');
    expect(appsStyles).toContain('.apps-panel--debloat .application-review-detail');
    expect(appsStyles).toContain('.app-content:has(.apps-panel--debloat)');
    expect(appsStyles).toContain('overflow-y: auto;');
  });

});
