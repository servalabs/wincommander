import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("Fleet Vault dismount surfaces", () => {
  test("Secure Storage lists only mounted Vaults the service says this admin may close", async () => {
    const [panel, section] = await Promise.all([
      Bun.file("src/panels/vault/index.tsx").text(),
      Bun.file("src/panels/vault/FleetMountedVaultsSection.tsx").text(),
    ]);

    expect(panel).toContain("<FleetMountedVaultsSection");
    expect(section).toContain('entry.mount_state === "mounted"');
    expect(section).toContain("entry.can_dismount");
    expect(section).toContain("listAuthorizedEntries()");
    expect(section).toContain("unmountEntry(entry.entry_id)");
    expect(section).not.toContain("dismountAllVolumes");
  });

  test("the emergency sidebar uses only the filtered service operation for encrypted volumes", async () => {
    const [sidebar, hook] = await Promise.all([
      Bun.file("src/components/RightSidebar.tsx").text(),
      Bun.file("src/hooks/useVaultAccess.ts").text(),
    ]);

    expect(hook).toContain('invoke<VaultAuthorizedDismountSummary>("vault_dismount_authorized_volumes")');
    expect(sidebar).toContain("await dismountAuthorizedVaults()");
    expect(sidebar).not.toContain("dismountAllVolumes(true)");
    expect(sidebar).toContain("authorized.denied > 0 || authorized.failed > 0");
  });
});
