import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("RDP machine-scope multi-user contracts", () => {
  test("gates incoming RDP for a standard user and requires Windows read-back", async () => {
    const source = await Bun.file("src/panels/privacy/RdpIdleCard.tsx").text();
    expect(source).toContain("Requires an administrator");
    expect(source).toContain("Blocked: needs-elevation");
    expect(source).toContain("const readIncomingStatus = useCallback");
    expect(source).toContain('executeBackendCommand<RdpIncomingReadBack>("Get-RdpIncomingIdleStatus")');
    expect(source).toContain("service read-back did not match");
    expect(source).toContain("Machine only · Windows account:");
    expect(source).toContain("disabled={applyingIncoming || needsElevation}");
  });

  test("writes and verifies the complete machine RDP policy and lock rule", async () => {
    const [rdp, lock] = await Promise.all([
      Bun.file("src-tauri/commander-free/scripts/modules/tweaks/system.ps1").text(),
      Bun.file("src-tauri/commander-free/scripts/modules/contingency/ops.ps1").text(),
    ]);
    for (const name of ["fDenyTSConnections", "MaxIdleTime", "MaxDisconnectionTime", "MaxConnectionTime", "fResetBroken"]) {
      expect(rdp).toContain(`-Name '${name}'`);
    }
    expect(rdp).toContain("Windows read-back did not match the requested RDP Incoming policy");
    expect(lock).toContain("Assert-IsAdmin");
    expect(lock).toContain("WC-LockRDP");
    expect(lock).toContain("Firewall read-back did not match the requested RDP lock rule");
  });

  test("uses per-user scheduler tasks and reports their actual task status", async () => {
    const source = await Bun.file("src/components/tweaks/managers/DiskCleanupGranular.tsx").text();
    expect(source).toContain("setMultiUserAutoEraseSchedule");
    expect(source).toContain("removeMultiUserAutoEraseSchedule");
    expect(source).toContain("owner {entry.ownerAccount ?? entry.targetUser");
    expect(source).toContain("signed-in account covered");
    expect(source).toContain("Blocked: needs-elevation");
  });

  test("keeps autostart limited and records the child stderr", async () => {
    const source = await Bun.file("src-tauri/commander-free/src/autostart.rs").text();
    expect(source).toContain("RunAsInvoker");
    expect(source).toContain("autostart.stderr.log");
    expect(source).toContain("-RunLevel Limited");
  });
});
