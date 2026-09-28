import { describe, expect, test } from "bun:test";
import { createRdpDiagnosticRecorder, type RdpDiagnosticArguments } from "./rdpDiagnostics";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

const safeEvent: RdpDiagnosticArguments = [
  "rdp-test", "session_monitor", "incoming_sessions_present", "verified", "succeeded", "info",
];

describe("RDP diagnostic-history preference", () => {
  test("disabled logging creates no RDP timeline write", () => {
    const writes: RdpDiagnosticArguments[] = [];
    createRdpDiagnosticRecorder(false, (...args) => writes.push(args))(...safeEvent);
    expect(writes).toEqual([]);
  });

  test("enabled logging writes the approved RDP event", () => {
    const writes: RdpDiagnosticArguments[] = [];
    createRdpDiagnosticRecorder(true, (...args) => writes.push(args))(...safeEvent);
    expect(writes).toEqual([safeEvent]);
  });

  test("one persisted preference gates every RDP timeline producer", async () => {
    const [app, card, outgoing, incomingSignout, incomingDismount, nativeWatch, writer, reader] = await Promise.all([
      Bun.file("src/App.tsx").text(),
      Bun.file("src/panels/privacy/RdpIdleCard.tsx").text(),
      Bun.file("src/hooks/useRdpIdleDisconnect.ts").text(),
      Bun.file("src/hooks/useRdpIncomingIdleSignout.ts").text(),
      Bun.file("src/hooks/useRdpIncomingDismount.ts").text(),
      Bun.file("src-tauri/commander-free/src/rdp_session_watch.rs").text(),
      Bun.file("src/lib/diagnostics.ts").text(),
      Bun.file("src/hooks/useDiagnosticCenter.ts").text(),
    ]);
    expect(app).toContain("rdpSaveLog === true");
    expect(app).toContain("rdpDiagnosticLoggingEnabled");
    expect(card).toContain("Record Remote Desktop activity in Diagnostics");
    expect(card).toContain("patchRdpTracking({ rdpSaveLog:");
    for (const source of [outgoing, incomingSignout, incomingDismount]) {
      expect(source).toContain("createRdpDiagnosticRecorder(diagnosticLoggingEnabled)");
    }
    expect(nativeWatch).toContain("rdp_save_log == Some(true)");
    expect(nativeWatch).toContain("if !rdp_diagnostics_enabled() {");
    expect(writer).toContain("new Event(DIAGNOSTIC_RECORDED_EVENT)");
    expect(reader).toContain("window.addEventListener(DIAGNOSTIC_RECORDED_EVENT");
    expect(reader).toContain("LIVE_REFRESH_FALLBACK_MS");
  });
});
