import { describe, expect, test } from "bun:test";
import { createRdpConsoleMirror, createRdpDiagnosticRecorder, type RdpDiagnosticArguments } from "./rdpDiagnostics";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

const safeEvent: RdpDiagnosticArguments = [
  "rdp-test", "session_monitor", "incoming_sessions_present", "verified", "succeeded", "info",
];

describe("RDP diagnostic-history logging", () => {
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

  test("console mirrors retain only an RDP state change", () => {
    const writes: RdpDiagnosticArguments[] = [];
    const mirror = createRdpConsoleMirror(true, (...args) => writes.push(args));
    mirror(["session_observation", "incoming_session_active", "verified", "succeeded", "info"]);
    mirror(["session_observation", "incoming_session_active", "verified", "succeeded", "info"]);
    mirror(["session_observation", "incoming_session_disconnected", "verified", "succeeded", "info"]);

    expect(writes).toHaveLength(2);
    expect(writes.map(([, action, stage]) => [action, stage])).toEqual([
      ["session_observation", "incoming_session_active"],
      ["session_observation", "incoming_session_disconnected"],
    ]);
    expect(writes.every((event) => event[7] === undefined)).toBe(true);
  });

  test("the existing application logging preference gates every frontend RDP timeline producer", async () => {
    const [app, card, outgoing, incomingSignout, incomingDismount, writer, reader] = await Promise.all([
      Bun.file("src/App.tsx").text(),
      Bun.file("src/panels/privacy/RdpIdleCard.tsx").text(),
      Bun.file("src/hooks/useRdpIdleDisconnect.ts").text(),
      Bun.file("src/hooks/useRdpIncomingIdleSignout.ts").text(),
      Bun.file("src/hooks/useRdpIncomingDismount.ts").text(),
      Bun.file("src/lib/diagnostics.ts").text(),
      Bun.file("src/hooks/useDiagnosticCenter.ts").text(),
    ]);
    expect(app).toContain("loggingEnabled !== false");
    expect(app).toContain("rdpDiagnosticLoggingEnabled");
    expect(card).not.toContain("Record Remote Desktop activity in Diagnostics");
    expect(card).not.toContain("rdpSaveLog");
    for (const source of [outgoing, incomingSignout, incomingDismount]) {
      expect(source).toContain("createRdpDiagnosticRecorder(diagnosticLoggingEnabled)");
      expect(source).toContain("createRdpConsoleMirror(diagnosticLoggingEnabled)");
    }
    expect(app).toContain("createRdpConsoleMirror(rdpDiagnosticLoggingEnabled)");
    expect(writer).toContain("new Event(DIAGNOSTIC_RECORDED_EVENT)");
    expect(reader).toContain("window.addEventListener(DIAGNOSTIC_RECORDED_EVENT");
    expect(reader).toContain("LIVE_REFRESH_DEBOUNCE_MS");
    expect(reader).toContain('window.addEventListener("focus", refreshWhenVisible)');
    expect(reader).not.toContain("setInterval");
  });
});
