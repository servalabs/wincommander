import { describe, expect, test } from "bun:test";
import { diagnosticBellProjection, type DiagnosticNotificationInput } from "./diagnosticNotification";

const baseEvent: DiagnosticNotificationInput = {
  feature: "vault", action: "mount", outcome: "failed", privacyClass: "local_sensitive",
};

describe("frontend diagnostics producer", () => {
  test("keeps diagnostic events structured and limits context to the writer allowlist", async () => {
    const source = await Bun.file("src/lib/diagnostics.ts").text();
    expect(source).toContain('invoke("record_diagnostic_event"');
    expect(source).toContain('const CONTEXT_KEYS');
    expect(source).toContain('"reason_category"');
    expect(source).toContain('"state"');
    expect(source).not.toContain("console.error");
    expect(source).toContain("diagnosticBellProjection");
    expect(source).toContain("pushNotification(projection.severity");
  });

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

  test("Privacy Shield, Flow, monitor, and Fleet producers use the shared writer", async () => {
    for (const path of [
      "src/panels/privacy/PrivacyShieldCard.tsx",
      "src/hooks/useFlowsV2.ts",
      "src/components/FlowActivityLogger.tsx",
      "src/components/BackgroundPollers.tsx",
    ]) {
      const source = await Bun.file(path).text();
      expect(source).toContain("recordDiagnostic");
    }
  });

  test("native monitor producers persist safe detections before UI delivery", async () => {
    for (const [path, code] of [
      ["src-tauri/commander-free/src/ransomware_monitor.rs", "RAN.DETECTION"],
      ["src-tauri/commander-free/src/paste_monitor.rs", "CLP.POLICY.DETECTED"],
      ["src-tauri/commander-free/src/vpn_kill_switch.rs", "VPN.TUNNEL.DROPPED"],
      ["src-tauri/commander-free/src/fleet_agent.rs", "FLT.POLICY.APPLY_FAILED"],
    ]) {
      const source = await Bun.file(path).text();
      expect(source).toContain("crate::diagnostics::record");
      expect(source).toContain(code);
    }
  });
});
