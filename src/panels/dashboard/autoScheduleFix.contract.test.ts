import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("Dashboard scheduled trace wipe finding", () => {
  test("routes Fix and Fix All through the idempotent scheduler and refreshes shared status", async () => {
    const dashboard = await Bun.file("src/panels/dashboard/index.tsx").text();

    expect(dashboard).toContain("f.id === 'auto-schedule-wipes'");
    expect(dashboard).toContain("ensureForensicTraceSchedules(setAutoEraseSchedule)");
    expect(dashboard).toContain("invalidateDiskCleanupScheduleStatus()");
    expect(dashboard).toContain("summary.firstFailure");
  });
});
