import { describe, expect, test } from "bun:test";
import { getAutoScheduleInterval, AUTO_SCHEDULE_INTERVALS, MANY_FINDINGS_THRESHOLD } from "./autoSchedulePolicy";

describe("automatic scheduled-wipe interval policy", () => {
  test("uses the frequent cadence for privacy traces and many normal findings", () => {
    expect(getAutoScheduleInterval({ id: "clipboardHistory", group: "standard" }, 0))
      .toBe(AUTO_SCHEDULE_INTERVALS.highFrequency);
    expect(getAutoScheduleInterval({ id: "thumbnailDb", group: "standard" }, MANY_FINDINGS_THRESHOLD))
      .toBe(AUTO_SCHEDULE_INTERVALS.manyFindings);
  });

  test("keeps normal traces at six hours and heavy/system cards daily", () => {
    expect(getAutoScheduleInterval({ id: "thumbnailDb", group: "standard" }, 2))
      .toBe(AUTO_SCHEDULE_INTERVALS.normal);
    expect(getAutoScheduleInterval({ id: "defenderHistory", group: "deep-dfir", systemWide: true }, 500))
      .toBe(AUTO_SCHEDULE_INTERVALS.heavy);
  });

  test("never violates a category's established minimum interval", () => {
    expect(getAutoScheduleInterval({ id: "clipboardHistory", group: "standard", minIntervalMinutes: 180 }, 0))
      .toBe(180);
  });
});
