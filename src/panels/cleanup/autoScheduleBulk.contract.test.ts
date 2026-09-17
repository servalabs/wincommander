import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("Auto-set scheduled wipes frontend contract", () => {
  test("uses only real schedulable cards and preserves existing schedules", async () => {
    const scan = await Bun.file("src/panels/cleanup/useCleanupScan.ts").text();

    expect(scan).toContain("SCHEDULABLE_CATEGORIES");
    expect(scan).toContain("if (!category.schedulable || !category.clearDataKey)");
    expect(scan).toContain("if (schedulesById[category.id] !== undefined)");
    expect(scan).toContain("summary.alreadyConfigured++");
    expect(scan).toContain('res.data?.status === "alreadyConfigured"');
    expect(scan).toContain("true,\n                    );");
  });

  test("uses category policy, the existing backend scheduler, and refreshes card clocks", async () => {
    const scan = await Bun.file("src/panels/cleanup/useCleanupScan.ts").text();

    expect(scan).toContain("getAutoScheduleInterval(");
    expect(scan).toContain("setAutoEraseSchedule(");
    expect(scan).toContain("!!category.schedulerRunAsSystem");
    expect(scan).toContain("await refreshSchedules()");
  });

  test("reports every outcome, including backend failures", async () => {
    const scan = await Bun.file("src/panels/cleanup/useCleanupScan.ts").text();

    expect(scan).toContain("created: 0");
    expect(scan).toContain("alreadyConfigured: 0");
    expect(scan).toContain("skipped: 0");
    expect(scan).toContain("failed: 0");
    expect(scan).toContain("formatAutoSetScheduleSummary(summary)");
    expect(scan).toContain("if (summary.failed > 0) showError(message)");
  });
});
