import { describe, expect, it } from "bun:test";
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
});
