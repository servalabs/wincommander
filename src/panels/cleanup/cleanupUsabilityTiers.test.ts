import { describe, expect, test } from "bun:test";
import {
  CLEANUP_USABILITY_TIERS,
  DEEP_DFIR_CATEGORIES,
  STANDARD_CATEGORIES,
} from "./cleanupCategories";
import { getCleanupScanConcurrency, runCleanupWorkers } from "./useCleanupScan";

describe("System Cleanup usability tiers", () => {
  test("assigns every scan category to one impact tier", () => {
    const tierIds = new Set(CLEANUP_USABILITY_TIERS.map((tier) => tier.id));

    for (const category of [...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES]) {
      expect(tierIds.has(category.usabilityTier!)).toBe(true);
    }
  });

  test("uses half the logical CPU count with a minimum of three scan workers", () => {
    expect(getCleanupScanConcurrency(1)).toBe(3);
    expect(getCleanupScanConcurrency(6)).toBe(3);
    expect(getCleanupScanConcurrency(8)).toBe(4);
    expect(getCleanupScanConcurrency(15)).toBe(7);
  });

  test("uses the scan worker limit for independent clears as well", async () => {
    let active = 0;
    let peak = 0;
    const completed: number[] = [];

    await runCleanupWorkers([1, 2, 3, 4, 5], async item => {
      active++;
      peak = Math.max(peak, active);
      await Promise.resolve();
      completed.push(item);
      active--;
    }, 2);

    expect(peak).toBe(2);
    expect(completed.sort()).toEqual([1, 2, 3, 4, 5]);
  });
});
