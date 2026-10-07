import { describe, expect, test } from "bun:test";
import { TWEAKS_TOGGLES } from "./tweaks.toggles";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("desktop shell priority", () => {
  test("uses a machine-wide, reversible, persistent toggle", () => {
    expect(TWEAKS_TOGGLES.find(entry => entry.id === "desktopShellPriority")).toMatchObject({
      needsAdmin: true,
      irreversible: false,
      settingsPath: "ideal.tweaks.os.desktopShellPriorityEnabled",
      currentPath: "current.tweaks.os.desktopShellPriorityEnabled",
      enableCmd: "Set-DesktopShellPriority",
      disableCmd: "Reset-DesktopShellPriority",
    });
  });

  test("validates persistence in the hidden backend host and never rolls it back because the live helper fails", async () => {
    const system = await Bun.file("src-tauri/commander-free/scripts/modules/tweaks/system.ps1").text();
    const settingsBridge = await Bun.file("src-tauri/commander-free/scripts/core/settings-bridge.ps1").text();

    expect(system).toContain("function Get-DesktopShellPriorityStatus");
    expect(system).toContain("function Wait-DesktopShellPriorityStatus");
    expect(system).toContain("function Test-ShellPriorityTaskOwned");
    expect(system).toContain("function Test-ShellPriorityPathSecure");
    expect(system).toContain("[IO.Directory]::GetAccessControl($Path)");
    expect(system).toContain("[IO.File]::GetAccessControl($Path)");
    expect(system).not.toContain("Get-Acl -LiteralPath $Path -ErrorAction Stop");
    expect(settingsBridge).toContain("[IO.File]::GetAccessControl($shellPriorityHelper)");
    expect(settingsBridge).not.toContain("Get-Acl -LiteralPath $shellPriorityHelper");
    expect(system).toContain("$script:LegacyShellPriorityTaskName = 'WinCommanderShellPriorityLogon'");
    expect(system).toContain("$status = Wait-DesktopShellPriorityStatus");
    expect(system).toContain("Task Scheduler does not always return a newly registered SYSTEM task");
    expect(system).toContain("$configurationConfirmed = $false");
    expect(system).toContain("Raising priorities for already-running shell processes is best-effort");
    expect(system).toContain("if (-not $configurationConfirmed) { Reset-DesktopShellPriority | Out-Null }");
    expect(system).toContain("if (-not $status.enabled) { throw 'Windows did not retain the desktop-shell priority configuration.' }");
    expect(system).toContain("verified = $true");
  });
});
