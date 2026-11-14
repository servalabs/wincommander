import { expect, test } from "bun:test";
import { maintenanceLogEvent } from "./maintenanceLogView";

test("maintenance failures stay visible while safe export fields omit paths and free text", () => {
  const event = maintenanceLogEvent({ id: "1", recordedAt: "2026-11-04T12:00:00Z", tool: "duplicates", outcome: "failed", title: "Failed C:\\private\\file", detail: "C:\\private\\file", affectedItems: 2 });
  expect(event.severity).toBe("error");
  expect(event.summary).toContain("C:\\private");
  expect(event.safeSummary).toBe("Duplicate cleanup · failed");
  expect(event.safeDetail).toBe("2 items");
  expect(`${event.safeSummary} ${event.safeDetail}`).not.toContain("private");
});

test("cancelled and partial maintenance results are warnings", () => {
  for (const outcome of ["partial", "cancelled"] as const) {
    const event = maintenanceLogEvent({ id: outcome, recordedAt: "2026-11-04T12:00:00Z", tool: "cleanup", outcome, title: "Cleanup" });
    expect(event.severity).toBe("warn");
  }
});
