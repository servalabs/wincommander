import { describe, expect, test } from "bun:test";
import { timeLabel } from "./ArgusSignalTable";

describe("ArgusSignalTable time labels", () => {
  test("keeps rendering historical signals without window timestamps", () => {
    expect(timeLabel({ windowStart: undefined as unknown as string, windowEnd: undefined as unknown as string }))
      .toBe("Unknown time");
  });

  test("formats complete signal windows", () => {
    expect(timeLabel({ windowStart: "2026-08-12T16:05:00Z", windowEnd: "2026-08-12T16:06:00Z" }))
      .toBe("2026-08-12 16:05–16:06");
  });
});
