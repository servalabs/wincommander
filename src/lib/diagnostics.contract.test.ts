import { describe, expect, test } from "bun:test";
import { diagnosticBellProjection, routeDiagnosticNotification, type DiagnosticNotificationInput } from "./diagnosticNotification";

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
    expect(source).toContain("routeDiagnosticNotification");
    expect(source).toContain("pushDiagnosticNotification(route.bell, operationId)");
    expect(source).not.toContain("pushNotification(");
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

  test("covered protected paths do not leave a DevTools-only or direct failure notification", async () => {
    const protectedPaths = [
      "src/components/BackgroundPollers.tsx",
      "src/components/FlowActivityLogger.tsx",
      "src/hooks/useAcquisitionWatch.ts",
      "src/hooks/useAuthAnomalyMonitor.ts",
      "src/hooks/useDecoyMonitor.ts",
      "src/hooks/useDistressPhrases.ts",
      "src/hooks/useLockdownWords.ts",
      "src/hooks/usePasteMonitor.ts",
      "src/hooks/useRansomwareMonitor.ts",
      "src/hooks/useRdpIdleDisconnect.ts",
      "src/hooks/useRdpIncomingDismount.ts",
      "src/hooks/useRdpIncomingIdleSignout.ts",
      "src/hooks/useRemoteAccessMonitor.ts",
      "src/hooks/useWifiGuardMonitor.ts",
      "src/panels/privacy/PasteMonitorSection.tsx",
      "src/panels/privacy/PrivacyShieldCard.tsx",
      "src/panels/privacy/RansomwareMonitorSection.tsx",
      "src/panels/privacy/RemoteAccessMonitorSection.tsx",
      "src/panels/privacy/UsbDevicesSection.tsx",
      "src/panels/vault/index.tsx",
    ];
    const failureOutput = /console\.(?:warn|error)\(|showError\(/g;

    for (const path of protectedPaths) {
      const source = await Bun.file(path).text();
      for (const match of source.matchAll(failureOutput)) {
        const start = Math.max(0, match.index! - 900);
        const nearby = source.slice(start, match.index! + 4_000);
        expect(nearby).toMatch(/record(?:Rdp)?Diagnostic|recordUsbFailure|\brecord\(/);
      }
    }
  });
});
