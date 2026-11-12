import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

const read = (path: string) => Bun.file(path).text();

describe("Packages and Apps layout", () => {
  test("labels the Winget-only bulk action separately from the combined update count", async () => {
    const installer = await read("src/panels/apps/components/AppInstallerPanel.tsx");
    expect(installer).toContain('collectUnifiedPackageUpdates(packageSnapshot.inventory?.managers ?? []');
    expect(installer).toContain('aria-label="Update all Winget app inventory updates"');
    expect(installer).toContain('Other package managers are updated in the Updates list.');
    expect(installer).not.toContain('installer-batch-action-label">UPDATE<br />ALL</span>');
  });
  test("keeps package updates inside Install software's Updates view", async () => {
    const panel = await read("src/panels/apps/index.tsx");
    const installer = await read("src/panels/apps/components/AppInstallerPanel.tsx");
    const updates = await read("src/panels/apps/PackageUpdateTools.tsx");

    expect(panel).not.toContain('value="updates-tools"');
    expect(panel).toContain("updatesTools={<PackageUpdateTools />}");
    expect(installer).toContain('<TabsContent value="updates">');
    expect(installer).toContain("{updatesTools ||");
    expect(installer).not.toContain('divider-label">FROM THE CATALOG');
    expect(installer).not.toContain('divider-label">OTHER PACKAGES');
    expect(panel).toContain('aria-label="Refresh installed apps and package updates"');
    expect(panel).toContain("refreshPackageAndAppInventories(");
    expect(panel).toContain("runAppInventoryScan(true, true)");
    expect(panel).toContain("backend.packageUpdatesInventory()");
    expect(panel).toContain('refreshPhaseRef.current !== "managers"');
    expect(panel).toContain('refreshPhase === "managers"');
    expect(panel).toContain("await cancelPromiseRef.current");
    expect(updates).not.toContain("Check for updates");
    expect(updates).not.toContain('aria-label="Package manager check results"');
    expect(updates).not.toContain('divider-label">OTHER PACKAGES');
  });

  test("keeps optional package managers out of engine readiness and removes obsolete package surfaces", async () => {
    const engines = await read("src/panels/apps/components/EnginesSection.tsx");
    const panel = await read("src/panels/apps/index.tsx");

    expect(engines).not.toContain('"chocolatey"');
    expect(engines).not.toContain('"scoop"');
    expect(engines).not.toContain('"encryptionEngine"');
    expect(panel).not.toContain("ClassicWindowsApps");
    expect(panel).not.toContain("classic-photo-viewer");
  });

  test("keeps Chocolatey and Scoop optional when unavailable", async () => {
    const updates = await read("src/panels/apps/PackageUpdateTools.tsx");
    const dependencies = await read("src-tauri/commander-free/scripts/modules/dependencies/dependencies.ps1");

    expect(updates).toContain('chocolatey: "Chocolatey"');
    expect(updates).toContain('scoop: "Scoop"');
    expect(updates).not.toContain('INSTALLABLE_MANAGERS');
    expect(dependencies).toContain("$Id -eq 'chocolatey' -or $Id -eq 'scoop'");
    expect(dependencies).not.toContain("Install-Chocolatey");
    expect(dependencies).not.toContain("Install-Scoop");
  });

  test("uses a compact update toolbar and offers one install action for missing optional managers", async () => {
    const updates = await read("src/panels/apps/PackageUpdateTools.tsx");
    const backend = await read("src/hooks/useBackend.ts");

    expect(updates).toContain('id="package-updates" className="flex scroll-mt-4 flex-col gap-4"');
    expect(updates).not.toContain('>App and package updates</h3>');
    expect(updates).toContain("unavailableOptionalManagers.length > 0");
    expect(updates.match(/packages\?\.managers\?\.filter/g)).toHaveLength(2);
    expect(updates).toContain("Install missing");
    expect(updates).toContain("packageUpdatesInstallOptionalManagers()");
    expect(updates).not.toContain("managerErrors");
    expect(updates).toContain("manager.available && manager.error");
    expect(updates).not.toContain("!manager.available || manager.error");
    expect(backend).toContain('invoke<PackageOptionalManagerInstallResult>("package_updates_install_optional_managers")');
  });

  test("moves successful installs to Installed without waiting for the inventory refresh", async () => {
    const installer = await read("src/panels/apps/components/AppInstallerPanel.tsx");
    const engines = await read("src/panels/apps/components/EnginesSection.tsx");

    expect(installer).toContain("setInstalledApps(prev => new Set(prev).add(id));");
    expect(installer).toContain('setInstallerView("installed")');
    expect(engines).not.toContain("eng-installed");
  });

  test("shows the full engine readiness view from the Engines category", async () => {
    const installer = await read("src/panels/apps/components/AppInstallerPanel.tsx");
    const engines = await read("src/panels/apps/components/EnginesSection.tsx");

    expect(installer).toContain('{ id: "engines", label: "Engines" }');
    expect(installer).toContain('selectedCategory === "engines"');
    expect(installer).toContain("<EnginesSection />");
    expect(engines).not.toContain("HIDDEN_FROM_ENGINES_GRID");
    expect(engines).toContain("Not installed ({missing.length})");
    expect(engines).toContain("Installed ({installed.length})");
  });

  test("includes the requested remote-support applications in the catalog", async () => {
    const manifest = await read("src-tauri/commander-free/scripts/modules/apps/winget.ps1");

    expect(manifest).toContain('id = "AnyDesk.AnyDesk"');
    expect(manifest).toContain('id = "DucFabulous.UltraViewer"');
    expect(manifest).toContain('id = "TeamViewer.TeamViewer"');
  });

  test("omits the retired encryption engine from catalog and engine filters", async () => {
    const manifest = await read("src-tauri/commander-free/scripts/modules/apps/winget.ps1");
    const installer = await read("src/panels/apps/components/AppInstallerPanel.tsx");

    expect(manifest).not.toContain('id = "IDRIX.VeraCrypt"');
    expect(installer).not.toContain('"IDRIX.VeraCrypt"');
  });
});
