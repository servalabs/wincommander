import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("DiagnosticEventBridge", () => {
  test("uses persisted Free, service, and Pro events as its only bell source", async () => {
    const source = await Bun.file("src/components/DiagnosticEventBridge.tsx").text();
    const ipcSource = await Bun.file("src/hooks/useDiagnosticsIpc.ts").text();
    expect(source).toContain("getDiagnosticEvents");
    expect(source).toContain("getServiceDiagnosticSummaries");
    expect(source).toContain("getProDiagnosticSummaries");
    expect(ipcSource).toContain('invoke<unknown[]>("get_diagnostic_events"');
    expect(ipcSource).toContain('invoke<unknown[]>("get_service_diagnostic_summaries"');
    expect(ipcSource).toContain('invoke<unknown[]>("get_pro_diagnostic_summaries"');
    expect(source).toContain("diagnosticBellProjection");
    expect(source).toContain("pushNotification");
    expect(source).toContain("CURSOR_KEY");
    expect(source).toContain("RETENTION_MS");
    expect(source).not.toContain("console.");
  });
});
