import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { ALL_CATEGORIES } from "./cleanupCategories";

function source(path: string) {
  const sourcePath = new URL(`../../${path}`, import.meta.url).pathname.replace(
    /^\/([A-Za-z]:\/)/,
    "$1",
  );
  // Normalise CRLF -> LF: the assertions below match multi-line source
  // snippets, and this repo has mixed line endings (git converts on checkout),
  // so a `"a\n  b"` expectation would fail on a CRLF checkout for a reason
  // unrelated to the coverage being enforced.
  return readFileSync(sourcePath, "utf8").replace(/\r\n/g, "\n");
}

describe("complete cleanup forensic viewer coverage", () => {
  test("keeps every scannable artifact connected to a real backend reader", async () => {
    const backendSource = [
      source("hooks/useBackend.ts"),
      source("lib/searchMaintenanceClient.ts"),
    ].join("\n");
    const scannable = ALL_CATEGORIES.filter((category) => category.getDataKey);

    expect(scannable.length).toBeGreaterThan(0);
    for (const category of scannable) {
      expect(new RegExp(`\\b${category.getDataKey}\\s*:`).test(backendSource)).toBe(true);
    }
  });

});
