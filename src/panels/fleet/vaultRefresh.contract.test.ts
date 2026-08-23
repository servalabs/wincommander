import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("Vault refresh boundaries", () => {
  test("mount lifecycle patches the returned row instead of refetching policy, status, and entries", async () => {
    const source = await Bun.file("src/panels/fleet/VaultAccessTab.tsx").text();
    const mountAction = source.slice(source.indexOf("const mountSelectedEntry"), source.indexOf("const unmountSelectedEntry"));
    const unmountAction = source.slice(source.indexOf("const unmountSelectedEntry"), source.indexOf("if (loading)"));

    expect(source).toContain("patchAuthorizedEntriesFromMountResult");
    expect(mountAction).not.toContain("refresh()");
    expect(unmountAction).not.toContain("refresh()");
  });

  test("dismount performs one parent-owned refresh after verification", async () => {
    const source = await Bun.file("src/panels/vault/VolumeActionsMenu.tsx").text();

    expect(source).not.toContain("refreshVault");
    expect(source).toContain("onDismounted();");
  });
});
