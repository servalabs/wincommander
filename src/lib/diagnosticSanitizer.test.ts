import { describe, expect, test } from "bun:test";
import { safeLegacyDiagnosticSource, safeLegacyDiagnosticSummary, sanitizeLegacyDiagnosticMessage } from "./diagnosticSanitizer";

describe("legacy diagnostic privacy boundary", () => {
  test("never returns legacy free text containing local or credential data", () => {
    const raw = "Vault C:\\Users\\Alice\\Taxes is mounted with token=super-secret and clipboard=payroll";
    const result = sanitizeLegacyDiagnosticMessage(raw, "core", "error");

    expect(result).toBe("Legacy CORE ERROR record");
    expect(result).not.toContain("Alice");
    expect(result).not.toContain("super-secret");
    expect(result).not.toContain("payroll");
  });

  test("uses bounded safe metadata when a legacy record is malformed", () => {
    expect(safeLegacyDiagnosticSummary("Alice's vault", "error: C:\\Users\\Alice")).toBe("Legacy LEGACY EVENT record");
    expect(safeLegacyDiagnosticSource("alice@example.com")).toBe("LEGACY");
  });
});
