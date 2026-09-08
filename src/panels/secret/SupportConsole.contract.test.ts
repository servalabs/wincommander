import { describe, expect, test } from "bun:test";

describe("Support Console privacy boundary", () => {
  test("queries the diagnostics broker without rendering sensitive context", async () => {
    const source = await Bun.file("src/panels/secret/SupportConsole.tsx").text();
    expect(source).toContain('invoke<DiagnosticEvent[]>("get_diagnostic_events"');
    expect(source).toContain('invoke<DiagnosticsHealth>("get_diagnostics_health")');
    expect(source).toContain('invoke<DiagnosticEvent[]>("get_service_diagnostic_summaries"');
    expect(source).not.toContain("redactedContext");
    expect(source).not.toContain("rawError");
    expect(source).toContain("This view never displays files, passwords, clipboard content, camera data, or raw system errors.");
  });
});
