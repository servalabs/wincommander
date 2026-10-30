import { describe, expect, test } from "bun:test";
import { appendMaintenanceOperation, MAINTENANCE_HISTORY_LIMIT, parseMaintenanceHistory, type MaintenanceOperation } from "./maintenanceHistory";

const entry: MaintenanceOperation = {
  id: "operation-1", recordedAt: "2026-10-21T08:00:00Z", tool: "cleanup",
  title: "Application cache cleanup", outcome: "partial", affectedItems: 2, bytesRecovered: 1024,
};

describe("maintenance operation display history", () => {
  test("preserves partial results and actual recovered bytes", () => {
    expect(parseMaintenanceHistory(JSON.stringify([entry]))).toEqual([entry]);
  });
  test("rejects corrupt, oversized, unknown and negative result data", () => {
    expect(parseMaintenanceHistory("{" )).toEqual([]);
    expect(parseMaintenanceHistory(" ".repeat(100_001))).toEqual([]);
    for (const invalid of [{ ...entry, outcome: "success" }, { ...entry, tool: "shell" }, { ...entry, bytesRecovered: -1 }, { ...entry, affectedItems: 0.5 }, { ...entry, recordedAt: "invalid" }]) {
      expect(parseMaintenanceHistory(JSON.stringify([invalid]))).toEqual([]);
    }
  });
  test("keeps the newest bounded entries without repeating an operation", () => {
    const previous = Array.from({ length: 150 }, (_, index) => ({ ...entry, id: String(index) }));
    const next = appendMaintenanceOperation(previous, { ...entry, id: "5" });
    expect(next).toHaveLength(MAINTENANCE_HISTORY_LIMIT);
    expect(next[0].id).toBe("5");
    expect(next.filter(item => item.id === "5")).toHaveLength(1);
  });
});
