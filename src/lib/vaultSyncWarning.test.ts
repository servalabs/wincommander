import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import {
  VAULT_SYNC_WARNING_EVENT, notifyPersonalMountSyncWarning, notifyPolicyMountSyncWarning,
  vaultSyncNotice, vaultSyncWarningMessage, type VaultSyncNotice,
} from "./vaultSyncWarning";

function capture() {
  const target = new EventTarget();
  const notices: VaultSyncNotice[] = [];
  target.addEventListener(VAULT_SYNC_WARNING_EVENT, event => notices.push((event as CustomEvent<VaultSyncNotice>).detail));
  return { target, notices };
}

describe("post-mount Syncthing warning", () => {
  it("reports stopped sync once without changing a successful personal mount", () => {
    const { target, notices } = capture();
    const result = { success: true, data: { status: "mounted", drive: "j:", syncWarning: "stopped" as const } };
    notifyPersonalMountSyncWarning(result, target);
    expect(notices).toEqual([{ drive: "J:", warning: "stopped" }]);
    expect(result.success).toBe(true);
    expect(result.data.status).toBe("mounted");
  });

  it("reports a confirmed Fleet or Quick Mount warning, not dismount or already-mounted failure", () => {
    const { target, notices } = capture();
    for (const state of ["failed", "denied", "unmounted", "unknown"]) {
      notifyPolicyMountSyncWarning({ state, drive_letter: "I:", sync_warning: "stopped" }, target);
    }
    expect(notices).toEqual([]);
    notifyPolicyMountSyncWarning({ state: "mounted", drive_letter: "I:", sync_warning: "unavailable" }, target);
    expect(notices).toEqual([{ drive: "I:", warning: "unavailable" }]);
  });

  it("asks for a recovery decision after a confirmed mount", () => {
    const { target, notices } = capture();
    notifyPolicyMountSyncWarning({ state: "mounted", drive_letter: "I:", sync_warning: "recovery_required" }, target);
    expect(notices).toEqual([{ drive: "I:", warning: "recovery_required" }]);
    expect(vaultSyncWarningMessage(notices[0])).toContain("remain paused");
  });

  it("does not warn for healthy, manually paused, unmanaged, missing, or failed receipts", () => {
    const { target, notices } = capture();
    notifyPersonalMountSyncWarning({ success: false, data: { status: "mounted", drive: "J:", syncWarning: "stopped" } }, target);
    notifyPersonalMountSyncWarning({ success: true }, target);
    notifyPersonalMountSyncWarning({ success: true, data: { status: "mounted", drive: "J:" } }, target);
    notifyPolicyMountSyncWarning({ state: "mounted", drive_letter: "J:", sync_warning: null }, target);
    expect(notices).toEqual([]);
  });

  it("accepts only bounded drive letters and closed warning codes", () => {
    expect(vaultSyncNotice("J", "stopped")).toEqual({ drive: "J:", warning: "stopped" });
    for (const drive of [null, "", "J:\\private", "bad", 7]) expect(vaultSyncNotice(drive, "stopped")).toBeNull();
    for (const warning of [null, "paused", "private error details", {}]) expect(vaultSyncNotice("J:", warning)).toBeNull();
  });

  it("explains the mounted container separately from stopped or unverified sync", () => {
    const stopped = vaultSyncWarningMessage({ drive: "J:", warning: "stopped" });
    expect(stopped).toContain("mounted successfully");
    expect(stopped).toContain("still stopped after an automatic rescan");
    expect(stopped).toContain("was not dismounted");
    const unavailable = vaultSyncWarningMessage({ drive: "J:", warning: "unavailable" });
    expect(unavailable).toContain("could not confirm");
    expect(unavailable).not.toContain("still stopped");
  });

  it("only presents a modal after readback and closing the initiating mount dialog", () => {
    const source = (path: string) => readFileSync(path, "utf8").replace(/\r\n/g, "\n");
    const expectBefore = (body: string, before: string, after: string) => {
      expect(body).toContain(before);
      expect(body).toContain(after);
      expect(body.indexOf(before) < body.indexOf(after)).toBe(true);
    };
    const storage = source("src/panels/vault/index.tsx");
    const storageMount = storage.slice(storage.indexOf("const result = await mountPasswordSelectedVolume"));
    expectBefore(storageMount, "const observationError = confirmedMountObservationError", "notifyPersonalMountSyncWarning(result)");
    expectBefore(storageMount, "setMountDialogOpen(false)", "notifyPersonalMountSyncWarning(result)");
    expect(storageMount).toContain("setMountedVolume(result.data.syncWarning ? null : result.data)");
    const fleet = source("src/panels/fleet/VaultAccessTab.tsx");
    const fleetMount = fleet.slice(fleet.indexOf("const result = await mountRequest"));
    expectBefore(fleetMount, "if (!vaultMountResultConfirmed", "notifyPolicyMountSyncWarning(result)");
    expectBefore(fleetMount, "setMountTarget(null)", "notifyPolicyMountSyncWarning(result)");
    const sidebar = source("src/components/RightSidebar.tsx");
    const quickFleet = sidebar.slice(sidebar.indexOf("const handleFleetVaultMount"), sidebar.indexOf("const nextFreeLetter"));
    expectBefore(quickFleet, "if (!confirmed || !vaultMountResultConfirmed", "notifyPolicyMountSyncWarning(result)");
    expect(quickFleet).toContain("setQmOpen(false);\n                    window.setTimeout(() => notifyPolicyMountSyncWarning(result), 350)");
    const quickPersonal = sidebar.slice(sidebar.indexOf("const handleQmMount"));
    expectBefore(quickPersonal, "const observationError = confirmedMountObservationError", "notifyPersonalMountSyncWarning(r)");
    expect(quickPersonal).toContain("setQmOpen(false);\n                    window.setTimeout(() => notifyPersonalMountSyncWarning(r), 350)");
    expect(source("src/hooks/useBackend.ts")).not.toContain("notifyPersonalMountSyncWarning");
    expect(source("src/hooks/useVaultAccess.ts")).not.toContain("notifyPolicyMountSyncWarning");
  });
});
