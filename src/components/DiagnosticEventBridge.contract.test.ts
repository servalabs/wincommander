import { describe, expect, test } from "bun:test";

describe("DiagnosticEventBridge", () => {
  test("uses persisted Free, service, and Pro events as its only bell source", async () => {
    const source = await Bun.file("src/components/DiagnosticEventBridge.tsx").text();
    expect(source).toContain('invoke<unknown[]>("get_diagnostic_events"');
    expect(source).toContain('invoke<unknown[]>("get_service_diagnostic_summaries"');
    expect(source).toContain('invoke<unknown[]>("get_pro_diagnostic_summaries"');
    expect(source).toContain("diagnosticBellProjection");
    expect(source).toContain("pushNotification");
    expect(source).toContain("CURSOR_KEY");
    expect(source).toContain("RETENTION_MS");
    expect(source).not.toContain("console.");
  });
});
