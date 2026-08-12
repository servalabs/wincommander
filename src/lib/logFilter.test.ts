import { describe, expect, test } from "bun:test";
import { filterLogRecords, levelFilterToBackendLevels, LEVEL_FILTERS, dedupNearSimultaneous, type LogRecord } from "./logFilter";

const rec = (level: string, source: string, message: string): LogRecord => ({
  date: "2026-06-16",
  timestamp: "12:00:00",
  level,
  source,
  message,
});

const RECORDS: LogRecord[] = [
  rec("ERROR", "pro", "[pro/wifi_guard] netsh failed"),
  rec("WARN", "core", "settings cache miss"),
  rec("INFO", "ui", "app started"),
  rec("ERROR", "ui", "unhandled rejection: boom"),
];

describe("filterLogRecords", () => {
  test("ALL/ALL/empty returns everything", () => {
    expect(filterLogRecords(RECORDS, "ALL", "ALL", "").length).toBe(4);
  });

  test("level filter keeps only that level", () => {
    const out = filterLogRecords(RECORDS, "ERROR", "ALL", "");
    expect(out.length).toBe(2);
    expect(out.every((r) => r.level === "ERROR")).toBe(true);
  });

  test("issue filter keeps warnings and errors", () => {
    const out = filterLogRecords(RECORDS, "ERROR_WARN", "ALL", "");
    expect(out.map((r) => r.level)).toEqual(["ERROR", "WARN", "ERROR"]);
  });

  test("source filter is case-insensitive against the lowercase source tag", () => {
    const out = filterLogRecords(RECORDS, "ALL", "PRO", "");
    expect(out.length).toBe(1);
    expect(out[0].source).toBe("pro");
  });

  test("search matches message substring case-insensitively", () => {
    const out = filterLogRecords(RECORDS, "ALL", "ALL", "NETSH");
    expect(out.length).toBe(1);
    expect(out[0].message).toContain("netsh");
  });

  test("level + source + search combine (AND)", () => {
    const out = filterLogRecords(RECORDS, "ERROR", "UI", "boom");
    expect(out.length).toBe(1);
    expect(out[0].source).toBe("ui");
    expect(out[0].level).toBe("ERROR");
  });

  test("non-matching search returns empty", () => {
    expect(filterLogRecords(RECORDS, "ALL", "ALL", "no-such-text").length).toBe(0);
  });

  test("backend levels are omitted only for all-level reads", () => {
    expect(levelFilterToBackendLevels("ALL")).toBe(undefined);
    expect(levelFilterToBackendLevels("ERROR_WARN")).toEqual(["error", "warn"]);
    expect(levelFilterToBackendLevels("INFO")).toEqual(["info"]);
  });

  test("debug is not exposed as a level filter", () => {
    expect(LEVEL_FILTERS).not.toContain("DEBUG");
  });
});

describe("dedupNearSimultaneous", () => {
  const at = (level: string, source: string, message: string, timestamp: string): LogRecord => ({
    date: "2026-06-16",
    timestamp,
    level,
    source,
    message,
  });

  test("collapses exact-duplicate same-source-level rows within the window", () => {
    // A single React-boundary crash logged verbatim twice: React's internal
    // echo + our own componentDidCatch record, one second apart, same
    // level+source+message.
    const out = dedupNearSimultaneous([
      at("ERROR", "ui", "The above error occurred in the Foo component: TypeError: boom", "12:00:01"),
      at("ERROR", "ui", "The above error occurred in the Foo component: TypeError: boom", "12:00:00"),
    ]);
    expect(out.length).toBe(1);
  });

  test("keeps the longer (richer) message when collapsing", () => {
    const out = dedupNearSimultaneous([
      at("ERROR", "ui", "same event message", "12:00:01"),
      at("ERROR", "ui", "same event message", "12:00:00"),
    ]);
    expect(out[0].message).toBe("same event message");
  });

  test("sums occurrences on merge instead of discarding the older record's count", () => {
    // Backend already summarized 9 identical errors (occurrences: 9); a 10th
    // arrives 1s later as its own record (occurrences: 1, the default). The
    // merged record must show 10, not silently drop the 9.
    const older = at("ERROR", "core", "disk write failed", "12:00:00");
    older.occurrences = 9;
    older.firstSeen = "2026-06-16 11:59:50";
    older.lastSeen = "2026-06-16 12:00:00";
    const newer = at("ERROR", "core", "disk write failed", "12:00:01");

    const out = dedupNearSimultaneous([newer, older]);
    expect(out.length).toBe(1);
    expect(out[0].occurrences).toBe(10);
  });

  test("does not merge distinct errors that share a long common prefix", () => {
    // Two different crashes from the same boundary sharing a long common
    // prefix but diverging afterward — must stay distinct, not collapse
    // into one because of a loose prefix match.
    const out = dedupNearSimultaneous([
      at(
        "ERROR",
        "ui",
        "[PanelErrorBoundary:files] Cannot read properties of undefined (reading 'length')",
        "12:00:01",
      ),
      at(
        "ERROR",
        "ui",
        "[PanelErrorBoundary:files] Cannot read properties of undefined (reading 'map')",
        "12:00:00",
      ),
    ]);
    expect(out.length).toBe(2);
  });

  test("does not merge across different sources", () => {
    const out = dedupNearSimultaneous([
      at("ERROR", "ui", "same prefix text here", "12:00:01"),
      at("ERROR", "pro", "same prefix text here but from pro", "12:00:00"),
    ]);
    expect(out.length).toBe(2);
  });

  test("does not merge across different levels", () => {
    const out = dedupNearSimultaneous([
      at("ERROR", "ui", "same prefix text here", "12:00:01"),
      at("WARN", "ui", "same prefix text here too", "12:00:00"),
    ]);
    expect(out.length).toBe(2);
  });

  test("does not merge the same message recurring outside the time window", () => {
    const out = dedupNearSimultaneous([
      at("ERROR", "ui", "same prefix text here", "12:00:10"),
      at("ERROR", "ui", "same prefix text here", "12:00:00"),
    ]);
    expect(out.length).toBe(2);
  });

  test("does not merge genuinely different near-simultaneous events", () => {
    const out = dedupNearSimultaneous([
      at("ERROR", "ui", "settings write failed", "12:00:01"),
      at("ERROR", "ui", "network probe timed out", "12:00:00"),
    ]);
    expect(out.length).toBe(2);
  });
});
