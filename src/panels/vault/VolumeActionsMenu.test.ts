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
  test("sets up distinct child sync folders and explains that pausing one does not stop Syncthing", () => {
    expect(source).toContain("validatePersonalVaultSyncFolders(syncFolders)");
    expect(source).toContain("Add another folder");
    expect(source).toContain("They cannot overlap");
    expect(source).toContain("Pausing a folder does not stop Syncthing");
    expect(source).toContain("for (const relativePath of validated.folders)");
  });
});
