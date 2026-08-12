import { test, expect } from "bun:test";
import { groupLogRecords, type LogRecord } from "../logFilter";

const rec = (message: string, timestamp: string, occurrences?: number): LogRecord =>
  ({ date: "2026-06-30", timestamp, level: "ERROR", source: "core", message, occurrences });

test("groups consecutive identical records with a summed count", () => {
  const grouped = groupLogRecords([rec("A", "10:04"), rec("A", "10:03"), rec("A", "10:01"), rec("B", "09:00")]);
  expect(grouped.length).toBe(2);
  expect(grouped[0].occurrences).toBe(3);
  expect(grouped[0].message).toBe("A");
  expect(grouped[1].occurrences).toBe(1);
});

test("does not merge across different messages", () => {
  const grouped = groupLogRecords([rec("A", "10:04"), rec("B", "10:03"), rec("A", "10:01")]);
  expect(grouped.map(g => g.message)).toEqual(["A", "B", "A"]);
});

test("honors wire-supplied occurrences from write-time summaries", () => {
  const grouped = groupLogRecords([rec("A", "10:04", 5), rec("A", "10:00", 2)]);
  expect(grouped.length).toBe(1);
  expect(grouped[0].occurrences).toBe(7);
});
