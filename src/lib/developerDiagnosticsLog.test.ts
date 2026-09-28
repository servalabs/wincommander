import { describe, expect, test } from "bun:test";
import {
  MAX_DEVELOPER_DIAGNOSTIC_ENTRIES,
  clearDeveloperDiagnosticsLog,
  configureDeveloperDiagnosticsLog,
  getDeveloperDiagnosticsSnapshot,
  recordDeveloperFlowDiagnostic,
} from "./developerDiagnosticsLog";

describe("developer diagnostics log", () => {
  test("requires native debug plus opt-in and clears when disabled", () => {
    configureDeveloperDiagnosticsLog(false, true);
    recordDeveloperFlowDiagnostic("FLOW notification emitted (severity info)");
    expect(getDeveloperDiagnosticsSnapshot()).toEqual([]);

    configureDeveloperDiagnosticsLog(true, true);
    recordDeveloperFlowDiagnostic("FLOW execution completed (flow Alice) after 42ms");
    expect(getDeveloperDiagnosticsSnapshot()).toHaveLength(1);
    expect(getDeveloperDiagnosticsSnapshot()[0]?.summary).toBe("FLOW execution completed after 42ms");

    configureDeveloperDiagnosticsLog(true, false);
    expect(getDeveloperDiagnosticsSnapshot()).toEqual([]);
  });

  test("rejects raw identifying text and keeps a bounded memory-only feed", () => {
    configureDeveloperDiagnosticsLog(true, true);
    recordDeveloperFlowDiagnostic("FLOW execution ended C:\\Users\\Alice\\secret.txt");
    recordDeveloperFlowDiagnostic("FLOW notification emitted https://example.invalid/private");
    expect(getDeveloperDiagnosticsSnapshot()).toEqual([]);

    for (let index = 0; index < MAX_DEVELOPER_DIAGNOSTIC_ENTRIES + 3; index += 1) {
      recordDeveloperFlowDiagnostic("FLOW notification emitted (severity info)");
    }
    expect(getDeveloperDiagnosticsSnapshot()).toHaveLength(MAX_DEVELOPER_DIAGNOSTIC_ENTRIES);
    clearDeveloperDiagnosticsLog();
  });
});
