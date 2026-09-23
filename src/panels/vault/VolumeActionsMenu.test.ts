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
});
