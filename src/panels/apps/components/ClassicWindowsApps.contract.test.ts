import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): {
    text(): Promise<string>;
  };
};

const read = (path: string) => Bun.file(path).text();

describe("Classic Windows apps contracts", () => {
  test("the panel exposes only the two supported photo experiences", async () => {
    const component = await read("src/panels/apps/components/ClassicWindowsApps.tsx");

    expect(component).toContain('name: "Windows Photo Viewer"');
    expect(component).toContain('name: "Photos Legacy"');
    expect(component).not.toContain('name: "Classic Paint"');
    expect(component).not.toContain('name: "Classic Snipping Tool"');
    expect(component).not.toContain('name: "Classic Notepad"');
  });

  test("availability checks expose every user-facing state and a manual retry", async () => {
    const component = await read("src/panels/apps/components/ClassicWindowsApps.tsx");
    const statusHook = await read("src/hooks/useClassicWindowsAppsStatus.ts");

    for (const label of ["Checking", "Available", "Not installed", "Check failed", "Check again", "Last checked"]) {
      expect(component).toContain(label);
    }
    expect(statusHook).toContain('setCheckState("failed")');
    expect(statusHook).toContain("catch (cause)");
  });

  test("Photo Viewer requires association values and working open commands", async () => {
    const statusModule = await read("src-tauri/commander-free/scripts/modules/tweaks/ai-control.ps1");
    const maintenanceModule = await read("src-tauri/commander-free/scripts/modules/tweaks/ai-control-maintenance.ps1");

    expect(statusModule).toContain("photoViewer = Test-AIControlPhotoViewerInstalled");
    expect(maintenanceModule).toContain("function Test-AIControlPhotoViewerInstalled");
    expect(maintenanceModule).toContain("Get-ItemPropertyValue -LiteralPath $capabilitiesPath");
    expect(maintenanceModule).toContain("GetValue('')");
    expect(maintenanceModule).toContain("ImageView_Fullscreen");
  });

  test("Photos Legacy is verified after the installer exits", async () => {
    const statusModule = await read("src-tauri/commander-free/scripts/modules/tweaks/ai-control.ps1");
    const maintenanceModule = await read("src-tauri/commander-free/scripts/modules/tweaks/ai-control-maintenance.ps1");

    expect(statusModule).toContain("photosLegacy = Test-AIControlPhotosLegacyInstalled");
    expect(maintenanceModule).toContain("function Test-AIControlPhotosLegacyInstalled");
    expect(maintenanceModule).toContain("The installer finished, but Windows did not register the Photos Legacy app package.");
    expect(maintenanceModule).toContain("Photos Legacy installation failed with exit code");
  });
});
