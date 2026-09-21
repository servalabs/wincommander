import { describe, expect, test } from "bun:test";
import { resolveIgnoredFindings } from "./IgnoredFindingsDialog";

describe("Ignored Fix All items", () => {
  test("uses a current finding label and keeps an unresolved saved ID manageable", () => {
    expect(resolveIgnoredFindings(
      ["privacy:telemetry", "stale:former-finding", "privacy:telemetry"],
      [{ id: "privacy:telemetry", label: "Block telemetry", impact: "Stops telemetry", category: "privacy", severity: "warning", safeDefault: true }],
    )).toEqual([
      { id: "privacy:telemetry", label: "Block telemetry", impact: "Stops telemetry" },
      { id: "stale:former-finding", label: "stale:former-finding", impact: undefined },
    ]);
  });
});
