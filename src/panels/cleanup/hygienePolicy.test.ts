import { describe, expect, test } from "bun:test";
import {
  DEEP_DFIR_CATEGORIES,
  STANDARD_CATEGORIES,
} from "./cleanupCategories";
import {
  ROUTINE_HYGIENE_CATEGORY_IDS,
  ROUTINE_HYGIENE_GUARDRAIL_IDS,
  buildRoutineHygienePlan,
  getRoutineHygieneCategories,
  isRoutineHygieneCategory,
  type HygieneCardSnapshot,
} from "./hygienePolicy";

const ALL_CATEGORIES = [...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES];

describe("Routine Hygiene policy", () => {
  test("resolves every allowlisted category to a clearable card", () => {
    const routine = getRoutineHygieneCategories(ALL_CATEGORIES);

    expect(routine.map(category => category.id)).toEqual(ROUTINE_HYGIENE_CATEGORY_IDS);
    for (const category of routine) {
      expect(category.actionOnly).not.toBe(true);
      expect(category.clearDataKey.length).toBeGreaterThan(0);
    }
  });

  test("never bulk-selects evidence, identity, recovery, or personal-data categories", () => {
    const guardedIds = Object.values(ROUTINE_HYGIENE_GUARDRAIL_IDS).flat();

    for (const categoryId of guardedIds) {
      expect(isRoutineHygieneCategory(categoryId)).toBe(false);
    }
  });

  test("fails closed for a future unknown category", () => {
    expect(isRoutineHygieneCategory("futureUnreviewedCleaner")).toBe(false);
  });

  test("requires a complete preview before bulk clearing", () => {
    const plan = buildRoutineHygienePlan(ALL_CATEGORIES, {});

    expect(plan.previewComplete).toBe(false);
    expect(plan.canClear).toBe(false);
    expect(plan.unscanned.length).toBe(ROUTINE_HYGIENE_CATEGORY_IDS.length);
  });

  test("fails closed when any reviewed preview reports an error", () => {
    const snapshots: Record<string, HygieneCardSnapshot> = Object.fromEntries(
      ROUTINE_HYGIENE_CATEGORY_IDS.map(id => [id, {
        count: 0,
        loading: false,
        clearing: false,
      }]),
    );
    snapshots.dnsCache.error = "backend unavailable";

    const plan = buildRoutineHygienePlan(ALL_CATEGORIES, snapshots);

    expect(plan.previewComplete).toBe(false);
    expect(plan.canClear).toBe(false);
    expect(plan.failed.map(category => category.id)).toEqual(["dnsCache"]);
  });

  test("selects only reviewed categories with positive findings", () => {
    const snapshots: Record<string, HygieneCardSnapshot> = Object.fromEntries(
      ROUTINE_HYGIENE_CATEGORY_IDS.map(id => [id, {
        count: 0,
        loading: false,
        clearing: false,
      }]),
    );
    snapshots.dnsCache.count = 4;
    snapshots.thumbnailDb.count = 2;

    const plan = buildRoutineHygienePlan(ALL_CATEGORIES, snapshots);

    expect(plan.previewComplete).toBe(true);
    expect(plan.canClear).toBe(true);
    expect(plan.totalFindings).toBe(6);
    expect(plan.ready.map(category => category.id)).toEqual(["dnsCache", "thumbnailDb"]);
  });
});
