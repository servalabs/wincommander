import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("Fleet Vault dismount surfaces", () => {
  test("Secure Storage lists only mounted Vaults the service says this account may close", async () => {
    const [panel, section] = await Promise.all([
      Bun.file("src/panels/vault/index.tsx").text(),
      Bun.file("src/panels/vault/FleetMountedVaultsSection.tsx").text(),
    ]);

    expect(panel).toContain("<FleetMountedVaultsSection");
    expect(section).toContain('entry.mount_state === "mounted"');
    expect(section).toContain("entry.can_dismount");
    expect(section).toContain("not Windows administrator status alone");
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
    expect(hook).toContain('invoke<PersonalVaultMountedVolume[]>("vault_list_personal_mounts")');
    expect(hook).toContain('"vault_dismount_personal_mount", { internalDrive, diagnosticOperationId: operationId }');
    expect(sidebar).toContain("await dismountAuthorizedVaults()");
    expect(sidebar).not.toContain("dismountAllVolumes(true)");
    expect(sidebar).toContain("Partial dismount.");
    expect(sidebar).toContain("finally {");
    expect(sidebar).toContain("await refreshVault(true)");
  });
});
