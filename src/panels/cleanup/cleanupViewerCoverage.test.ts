import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { buildTraceView } from "../../components/shared/traceTable";
import {
  ACTION_CATEGORIES,
  ALL_CATEGORIES,
  CLEANUP_USABILITY_TIERS,
  VIEW_ONLY_CATEGORIES,
} from "./cleanupCategories";

function source(path: string) {
  const sourcePath = new URL(`../../${path}`, import.meta.url).pathname.replace(
    /^\/([A-Za-z]:\/)/,
    "$1",
  );
  return readFileSync(sourcePath, "utf8");
}

describe("complete cleanup forensic viewer coverage", () => {
  test("keeps every scannable artifact connected to a real backend reader", async () => {
    const backendSource = source("hooks/useBackend.ts");
    const scannable = ALL_CATEGORIES.filter((category) => category.getDataKey);

    expect(ALL_CATEGORIES).toHaveLength(84);
    expect(scannable).toHaveLength(81);
    for (const category of scannable) {
      expect(new RegExp(`\\b${category.getDataKey}\\s*:`).test(backendSource)).toBe(true);
    }
  });

  test("accounts for every category across all five rendered tabs", () => {
    const tierCounts = Object.fromEntries(
      CLEANUP_USABILITY_TIERS.map((tier) => [
        tier.id,
        ALL_CATEGORIES.filter((category) => category.usabilityTier === tier.id).length,
      ]),
    );

    expect(tierCounts).toEqual({
      "low-impact": 18,
      "history-cache": 23,
      "rebuilds-apps-connectivity": 13,
      "data-accounts-recovery": 26,
    });
    expect(Object.values(tierCounts).reduce((total, count) => total + count, 0)).toBe(80);
    expect(ACTION_CATEGORIES.map((category) => category.id)).toEqual([
      "virtualMemory",
      "unallocatedErase",
      "previousWindowsInstall",
    ]);
    expect(VIEW_ONLY_CATEGORIES.map((category) => category.id)).toEqual(["processIntel"]);
    expect(ALL_CATEGORIES.filter((category) => category.getDataKey && !category.clearDataKey).map((category) => category.id)).toEqual(["processIntel"]);
  });

  test("renders every scannable artifact as a multi-column forensic dataset", () => {
    const scannable = ALL_CATEGORIES.filter((category) => category.getDataKey);

    for (const category of scannable) {
      const view = buildTraceView({
        total: 2,
        records: [
          {
            artifactId: `${category.id}-001`,
            category: category.label,
            source: category.getDataKey,
            path: `C:\\Audit\\${category.id}\\artifact-one.dat`,
            timestamp: "2026-08-01 22:31:08",
            count: 1,
          },
          {
            artifactId: `${category.id}-002`,
            category: category.label,
            source: category.getDataKey,
            path: `HKCU\\Software\\WinCommanderAudit\\${category.id}`,
            timestamp: "2026-08-01 21:14:52",
            count: 1,
          },
        ],
      }, []);

      expect(view.datasets.length).toBeGreaterThan(0);
      for (const dataset of view.datasets) {
        expect(dataset.columns.length).toBeGreaterThanOrEqual(2);
        expect(dataset.rows.length).toBeGreaterThan(0);
      }
    }
  });

  test("keeps card and dialog actions category-specific for exhaustive automation", async () => {
    const card = source("components/cleanup/CleanupTraceCard.tsx");
    const schedule = source("components/cleanup/CleanupScheduleControl.tsx");
    const dialog = source("components/shared/TraceDetailDialog.tsx");

    expect(card).toContain("data-cleanup-category={category.id}");
    expect(card).toContain("data-cleanup-state={bandState}");
    expect(card).toContain('aria-label={`${category.label}: ${accessibleStatus}`}');
    expect(card).toContain("aria-label={`View details for ${category.label}`}");
    expect(card).toContain("aria-label={`Scan ${category.label}`}");
    expect(card).toContain("aria-label={`Rescan ${category.label}`}");
    expect(card).toContain("actionLabel={category.label}");
    expect(schedule).toContain("categoryLabel: string");
    expect(schedule).toContain("`Schedule auto-clean for ${categoryLabel}`");
    expect(dialog).toContain("`Dismiss ${category.label} details`");
    expect(dialog).toContain("`Close ${category.label} details`");
    expect(dialog).toContain("`Copy ${dataset.title} as TSV`");
    expect(dialog).toContain("`Sort ${dataset.title} by ${column}`");
  });
});
