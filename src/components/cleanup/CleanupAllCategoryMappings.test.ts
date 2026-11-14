import { describe, expect, test } from "bun:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { ALL_CATEGORIES } from "../../panels/cleanup/cleanupCategories";
import CleanupRecordPreview, { getSafeCleanupPreview } from "./CleanupRecordPreview";

describe("System Cleanup record mappings", () => {
  for (const category of ALL_CATEGORIES.filter(item => item.requiredScanArrays?.length)) {
    test(`${category.label} retains returned record fields`, () => {
      const marker = `field-${category.id}`;
      const record = { name: marker, path: `C:\\fixture\\${category.id}` };
      const key = category.requiredScanArrays![0];
      const raw = category.id === "amcache"
        ? { entries: [{ category: "InventoryApplication", count: 1, sample: [record] }] }
        : category.id === "ntUserTraces"
          ? { sections: [{ name: "RunMRU", count: 1, entries: [record] }] }
          : { [key]: [record] };
      const rows = getSafeCleanupPreview(category.id, raw);
      expect(rows.length).toBeGreaterThan(0);
      expect(rows.flatMap(row => row.facts).join("\n")).toContain(marker);
      const html = renderToStaticMarkup(createElement(CleanupRecordPreview, {
        categoryId: category.id, mode: "advanced", rawData: raw,
      }));
      expect(html).toContain(marker);
    });
  }

  test("explains a record with no returned details beside populated records", () => {
    const html = renderToStaticMarkup(createElement(CleanupRecordPreview, {
      categoryId: "dnsCache", mode: "advanced", rawData: { entries: [{ name: "example.test" }, {}] },
    }));
    expect(html).toContain("example.test");
    expect(html).toContain("No record details returned");
  });
});
