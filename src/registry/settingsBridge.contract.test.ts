import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("settings bridge ownership", () => {
  test("PowerShell probes do not maintain a plaintext settings shadow", async () => {
    const [bridge, dependencies, productivity, browserSecurity] = await Promise.all([
      Bun.file("src-tauri/commander-free/scripts/core/settings-bridge.ps1").text(),
      Bun.file("src-tauri/commander-free/scripts/modules/dependencies/dependencies.ps1").text(),
      Bun.file("src-tauri/commander-free/scripts/modules/productivity.ps1").text(),
      Bun.file("src-tauri/commander-free/scripts/modules/tweaks/security.ps1").text(),
    ]);

    for (const staleApi of [
      "Get-WCSettingsPath",
      "Get-WCSettings",
      "Set-WCSetting",
      "Get-WCSetting",
      "Test-WCSettingLocked",
      "Sync-WCSettingFromRegistry",
    ]) {
      expect(bridge).not.toContain(staleApi);
    }
    expect(bridge).not.toContain('Set-Content -Path $settingsPath');
    expect(dependencies).not.toContain("Set-WCSetting");
    expect(dependencies).not.toContain("Invoke-ProductivityEngineMaintenance");
    expect(productivity).not.toContain("Get-WCSetting");
    expect(browserSecurity).not.toContain("Get-WCSetting");
  });
});
