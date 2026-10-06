import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const source = readFileSync("src/panels/vault/VolumeActionsMenu.tsx", "utf8");

describe("encrypted-volume dismount", () => {
  test("force-dismounts the exact engine slot and removes a row only after status readback", () => {
    expect(source).toContain("dismountVolume(letter, true, internalDrive)");
    expect(source).toContain("await completeDismount(true)");
    expect(source).toContain("getEncryptedVolumeStatus()");
    expect(source).toContain("if (!stillMounted) return null;");
    expect(source).toContain('content="Force dismount"');
    expect(source).not.toContain("handleForceDismount");
    expect(source).not.toContain("forceConfirmOpen");
  });
  test("reports row errors outside the narrow action column and preserves service permission hints", () => {
    const panel = readFileSync("src/panels/vault/index.tsx", "utf8");
    expect(source).toContain("onErrorChange?.(");
    expect(source).toContain("aria-description={permissionHint}");
    expect(panel).toContain('className="vault-volume-feedback-row"><td colSpan={4}>');
    expect(panel).toContain("onErrorChange={setVolumeActionFailure}");
    expect(panel).toContain("loading={refreshing}");
    expect(panel).toContain("disabled={refreshing}");
    expect(panel).toContain("onClick={() => refreshVault(false)}");
  });
  test("cloud opens explicit per-Vault management without enrolling during inspection", () => {
    const dialog = readFileSync("src/components/shared/PersonalVaultSyncDialog.tsx", "utf8");
    expect(source).toContain("<PersonalVaultSyncDialog");
    expect(source).toContain("setSyncSetupOpen(true)");
    expect(source).not.toContain("enablePersonalVaultSync(");
    expect(dialog).toContain('managePersonalVaultSync(internalDrive, "list")');
    expect(dialog).toContain("Sync is not enabled for this Vault");
    expect(dialog).toContain("Other Syncthing folders keep running");
    expect(dialog).toContain("Enable selected folders");
    expect(dialog).toContain("Confirm remove sync");
    expect(dialog).toContain('"remove", folder.relative_path, undefined, folder.folder_id');
  });
});
