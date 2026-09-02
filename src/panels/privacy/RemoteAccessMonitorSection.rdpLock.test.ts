import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("incoming RDP lock control", () => {
  test("uses the machine-only typed service operation and verifies its observed state", async () => {
    const source = await Bun.file("src/panels/privacy/RemoteAccessMonitorSection.tsx").text();

    expect(source).toContain('setting: "rdp_lock"');
    expect(source).toContain('value: { kind: "rdp_lock", locked }');
    expect(source).toContain('observed.kind === "rdp_lock" && observed.locked === locked');
    expect(source).toContain("service read-back did not match the requested RDP lock state");
    expect(source).toContain("Machine only");
  });

  test("blocks standard users before the machine setting is dispatched", async () => {
    const source = await Bun.file("src/panels/privacy/RemoteAccessMonitorSection.tsx").text();

    expect(source).toContain("Blocked: needs-elevation");
    expect(source).toContain("Requires an administrator");
    expect(source).toContain("disabled={applyingRdpLock || needsElevation}");
  });

  test("a stopped/unregistered WinCommanderSvc offers repair instead of the raw pipe error", async () => {
    const source = await Bun.file("src/panels/privacy/RemoteAccessMonitorSection.tsx").text();

    // Matches svc_client::call's `format!("service connect failed: {e}")`
    // thrown when the named pipe has no listening service (e.g. "os error 2").
    expect(source).toContain('message.toLowerCase().includes("service connect failed")');
    expect(source).toContain("The WinCommander system service isn't running");
    expect(source).toContain('invoke<string>("repair_commander_service")');
    expect(source).toContain("rdpLockServiceDown && !needsElevation");
  });
});
