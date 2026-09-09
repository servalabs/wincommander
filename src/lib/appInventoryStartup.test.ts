import { describe, expect, test } from "bun:test";
import { isAppInventoryRefreshDue } from "./appInventoryStartup";

describe("isAppInventoryRefreshDue", () => {
  const now = Date.parse("2026-09-09T06:00:00.000Z");

  test("reuses a recent cached inventory at startup", () => {
    expect(isAppInventoryRefreshDue("2026-09-09T05:45:01.000Z", now)).toBe(false);
  });

  test("refreshes a missing, invalid, future, or stale inventory", () => {
    expect(isAppInventoryRefreshDue(undefined, now)).toBe(true);
    expect(isAppInventoryRefreshDue("not-a-date", now)).toBe(true);
    expect(isAppInventoryRefreshDue("2026-09-09T06:01:00.000Z", now)).toBe(true);
    expect(isAppInventoryRefreshDue("2026-09-09T05:30:00.000Z", now)).toBe(true);
  });
});
