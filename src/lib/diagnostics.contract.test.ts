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

  test("frontend monitor and RDP enforcement failures use durable safe diagnostics", async () => {
    for (const [path, failureCode] of [
      ["src/hooks/useWifiGuardMonitor.ts", "WIFI.GUARD.START_FAILED"],
      ["src/hooks/useAuthAnomalyMonitor.ts", "AUTH.MONITOR.START_FAILED"],
      ["src/hooks/useRemoteAccessMonitor.ts", "REMOTE.ACCESS.START_FAILED"],
      ["src/hooks/useRdpIncomingIdleSignout.ts", "RDP.SESSION.READBACK_FAILED"],
      ["src/hooks/useRdpIncomingDismount.ts", "RDP.SESSION.READBACK_FAILED"],
      ["src/hooks/useRdpIdleDisconnect.ts", "RDP.IDLE.PROBE_FAILED"],
    ]) {
      const source = await Bun.file(path).text();
      expect(source).toMatch(/record(?:Rdp)?Diagnostic/);
      expect(source).toContain(failureCode);
    }
  });

  test("background monitor reconciliation and failure paths persist safe diagnostics", async () => {
    for (const [path, requiredCodes] of [
      ["src/hooks/useDecoyMonitor.ts", ["DEC.MONITOR.RECONCILE_FAILED", "recordDiagnostic"]],
      ["src/hooks/useDistressPhrases.ts", ["PRV.DISTRESS.SYNC_FAILED", "recordDiagnostic"]],
      ["src/hooks/useLockdownWords.ts", ["LCK.MONITOR.START_FAILED", "recordDiagnostic"]],
      ["src/hooks/usePasteMonitor.ts", ["CLP.MONITOR.START_FAILED", "recordDiagnostic"]],
      ["src/hooks/useRansomwareMonitor.ts", ["RAN.MONITOR.RECONCILE_FAILED", "recordDiagnostic"]],
      ["src/hooks/useAcquisitionWatch.ts", ["VLT.ACQUISITION.DISMOUNT_FAILED", "recordDiagnostic"]],
    ] as const) {
      const source = await Bun.file(path).text();
      for (const required of requiredCodes) expect(source).toContain(required);
    }

    const background = await Bun.file("src/components/BackgroundPollers.tsx").text();
    for (const action of [
      "paste_detection", "file_detection", "mass_modification_detection",
      "coercion_phrase_detection", "session_detection", "screen_capture_detection",
      "health_detection", "rogue_access_point_detection", "honeypot_detection",
    ]) expect(background).toContain(action);
    expect(background).toContain('privacyClass: "restricted"');
    expect(background).not.toContain("context: { fullPath");
    expect(background).not.toContain("context: { peer");
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
