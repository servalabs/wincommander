import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("Vault refresh boundaries", () => {
  test("saving invalidates old access before applying and requires a verified refresh", async () => {
    const source = await Bun.file("src/panels/fleet/VaultAccessTab.tsx").text();
    const apply = source.slice(source.indexOf("const apply ="), source.indexOf("const importLegacyDraft"));

    expect(apply.indexOf("++refreshRevision.current") < apply.indexOf("await applyPolicy")).toBe(true);
    expect(apply.indexOf("setAuthorizedEntries([])") < apply.indexOf("await applyPolicy")).toBe(true);
    expect(apply).toContain("const refreshed = await refresh(true, false)");
    expect(apply).toContain("if (!refreshed)");
    expect(apply).toContain("if (saveInProgress.current) return");
    expect(source).toContain('<fieldset disabled={saving} className="contents">');
  });

  test("refresh rejects stale responses and clears obsolete mount results", async () => {
    const source = await Bun.file("src/panels/fleet/VaultAccessTab.tsx").text();
    const refresh = source.slice(source.indexOf("const refresh ="), source.indexOf("useEffect(() => { void refresh()"));

    expect(refresh).toContain("revision !== refreshRevision.current");
    expect(refresh).toContain("setMountResults({})");
    const failure = refresh.slice(refresh.indexOf("} catch"));
    expect(failure).toContain("setAuthorizedEntries([])");
  });

  test("the editor mounts the saved authorized container type instead of unsaved fields", async () => {
    const source = await Bun.file("src/panels/fleet/VaultAccessTab.tsx").text();
    expect(source).toContain("if (authorized) openMountPrompt(authorized)");
    expect(source).not.toContain("openMountPrompt({ ...entry");
  });

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
