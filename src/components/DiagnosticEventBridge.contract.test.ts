import { describe, expect, test } from "bun:test";
import { asEventList } from "./DiagnosticEventBridge";

describe("DiagnosticEventBridge", () => {
  test("ignores non-array responses from unavailable native commands", () => {
    expect(asEventList({ success: false, error: "native backend unavailable" })).toEqual([]);
    expect(asEventList(undefined)).toEqual([]);
    expect(asEventList([{ eventId: "event-1" }])).toEqual([{ eventId: "event-1" }]);
  });
});
