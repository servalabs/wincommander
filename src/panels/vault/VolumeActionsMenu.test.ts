import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const source = readFileSync("src/panels/vault/VolumeActionsMenu.tsx", "utf8");

describe("encrypted-volume dismount", () => {
  test("uses the authenticated service route for personal and Fleet mounts", () => {
    expect(source).toContain("dismountTarget.kind === \"fleet\"");
    expect(source).toContain("await unmountEntry(dismountTarget.entryId)");
    expect(source).toContain("await dismountPersonalMount(dismountTarget.internalDrive)");
    expect(source).toContain("await completeDismount()");
    expect(source).not.toContain("dismountVolume(letter");
    expect(source).toContain("getEncryptedVolumeStatus()");
    expect(source).toContain("if (!stillMounted) return null;");
    expect(source).toContain("if (!dismountTarget) return;");
    expect(source).toContain("disabled={dismountUnavailable}");
  });
});
