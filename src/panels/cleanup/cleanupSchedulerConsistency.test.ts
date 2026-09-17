import { describe, expect, test } from "bun:test";

import {
  ACTION_CATEGORIES,
  STANDARD_CATEGORIES,
  DEEP_DFIR_CATEGORIES,
  SUPPORTED_AUTOERASE_IDS,
  VIEW_ONLY_CATEGORIES,
} from "./cleanupCategories";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("System Cleanup scheduler catalogue", () => {
  test("every schedulable card has a real shared scheduler payload", async () => {
    const scheduler = await Bun.file("src-tauri/wincmd-shared/scripts/auto-erase.ps1").text();
    const scriptIds = new Set(
      Array.from(scheduler.matchAll(/^\s*'([^']+)'\s*=/gm), (match) => match[1]),
    );

    // clipboardHistory is deliberately a UI name; its persisted task uses the
    // stable backend id `clipboard` so previously-created tasks continue to
    // appear on the same card.
    for (const category of [...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES]) {
      if (!category.schedulable) continue;
      const schedulerId = category.schedulerCategoryId ?? category.id;
      expect(scriptIds.has(schedulerId)).toBe(true);
      expect(SUPPORTED_AUTOERASE_IDS.has(schedulerId)).toBe(true);
    }
  });

  test("action and view-only cards cannot expose a scheduled wipe", () => {
    // These actions either change system configuration, overwrite drive space,
    // remove recovery data, or merely inspect state. None is safe to execute
    // repeatedly without an explicit fresh user action.
    for (const category of [...ACTION_CATEGORIES, ...VIEW_ONLY_CATEGORIES]) {
      expect(category.schedulable).not.toBe(true);
      expect(category.schedulerCategoryId).toBeUndefined();
      expect(SUPPORTED_AUTOERASE_IDS.has(category.id)).toBe(false);
    }
  });
});
