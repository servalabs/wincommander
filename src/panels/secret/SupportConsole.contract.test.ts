import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("Diagnostic Center privacy boundary", () => {
  test("combines structured and legacy records without rendering sensitive context", async () => {
    const [source, hook] = await Promise.all([
      Bun.file("src/panels/secret/SupportConsole.tsx").text(),
      Bun.file("src/hooks/useDiagnosticCenter.ts").text(),
    ]);
    expect(hook).toContain('invoke<DiagnosticEvent[]>("get_diagnostic_events"');
    expect(hook).toContain('invoke<DiagnosticsHealth>("get_diagnostics_health")');
    expect(hook).toContain('invoke<DiagnosticEvent[]>("get_service_diagnostic_summaries"');
    expect(hook).toContain('invoke<LogRecord[]>("get_log_records"');
    expect(source).toContain("sanitizeLegacyDiagnosticMessage(record.message");
    expect(source).toContain("Copy safe text");
    expect(source).toContain("aria-label=\"Unified diagnostic timeline\"");
    expect(source).not.toContain("redactedContext");
    expect(source).not.toContain("rawError");
    expect(source).toContain("Legacy free-text details are intentionally hidden");
  });
});
