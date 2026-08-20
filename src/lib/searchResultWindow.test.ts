import { describe, expect, test } from "bun:test";
import { buildContentQueryArgs } from "./contentSearch";
import { calculateSearchResultWindow, SEARCH_RESULT_WINDOW_LIMIT } from "./searchResultWindow";

describe("calculateSearchResultWindow", () => {
  test("keeps filename and content search rows within the 100-row DOM budget", () => {
    const window = calculateSearchResultWindow({
      itemCount: 2_000,
      scrollTop: 18_000,
      viewportHeight: 480,
      rowHeight: 36,
    });

    expect(window.end - window.start).toBe(SEARCH_RESULT_WINDOW_LIMIT);
    const contentLimit = buildContentQueryArgs("budget").limit ?? 0;
    expect(SEARCH_RESULT_WINDOW_LIMIT + contentLimit <= 100).toBe(true);
  });

  test("keeps a keyboard-selected row mounted when it is outside the scroll window", () => {
    const window = calculateSearchResultWindow({
      itemCount: 2_000,
      scrollTop: 0,
      viewportHeight: 480,
      rowHeight: 36,
      pinnedIndex: 1_750,
    });

    expect(window.start <= 1_750).toBe(true);
    expect(window.end).toBeGreaterThan(1_750);
    expect(window.end - window.start <= SEARCH_RESULT_WINDOW_LIMIT).toBe(true);
  });
});
