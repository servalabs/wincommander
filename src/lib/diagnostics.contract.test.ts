import { describe, expect, test } from "bun:test";
import { diagnosticBellProjection, routeDiagnosticNotification, type DiagnosticNotificationInput } from "./diagnosticNotification";

const baseEvent: DiagnosticNotificationInput = {
  feature: "vault", action: "mount", outcome: "failed", privacyClass: "local_sensitive",
};

describe("frontend diagnostics producer", () => {
  test("projects only terminal attention outcomes into a safe bell message", () => {
    expect(diagnosticBellProjection(baseEvent)).toEqual({
      severity: "danger", kind: "notification", message: "Vault mount failed",
    });
    expect(diagnosticBellProjection({ ...baseEvent, outcome: "degraded" })).toEqual({
      severity: "warn", kind: "notification", message: "Vault mount needs attention",
    });
    expect(diagnosticBellProjection({ ...baseEvent, outcome: "succeeded" })).toBeUndefined();
  });

  test("never puts restricted detail or caller-controlled tokens in the bell", () => {
    const projection = diagnosticBellProjection({
      ...baseEvent, feature: "alice_private_vault", action: "C_users_alice_secret", privacyClass: "restricted",
      outcome: "timed_out",
    });
    expect(projection).toEqual({ severity: "danger", kind: "notification", message: "A protected operation failed" });
    expect(JSON.stringify(projection)).not.toContain("alice");
  });

  test("disables Windows notifications by default and permits only generic critical protection text", () => {
    expect(routeDiagnosticNotification({
      ...baseEvent, feature: "vault", severity: "critical",
    }).windows).toEqual({ mode: "disabled" });
    expect(routeDiagnosticNotification({
      ...baseEvent, feature: "ransomware", action: "detection", severity: "critical", privacyClass: "public",
    }).windows).toEqual({
      mode: "generic_critical", message: "WinCommander protection needs attention",
    });
    expect(routeDiagnosticNotification({
      ...baseEvent, feature: "ransomware", action: "alice_secret_path", severity: "critical", privacyClass: "restricted",
    }).windows).toEqual({ mode: "disabled" });
  });

  test("Fleet policy never exposes local diagnostic detail", () => {
    expect(routeDiagnosticNotification({ ...baseEvent, privacyClass: "local_sensitive" }).fleet).toBe("withhold_local_detail");
    expect(routeDiagnosticNotification({ ...baseEvent, privacyClass: "restricted" }).fleet).toBe("withhold_local_detail");
    expect(routeDiagnosticNotification({ ...baseEvent, privacyClass: "public" }).fleet).toBe("managed_outcome_only");
  });
});
