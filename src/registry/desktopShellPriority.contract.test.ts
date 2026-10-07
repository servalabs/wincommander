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

  test("waits for the scheduler readback before rolling back a persistent configuration", async () => {
    const system = await Bun.file("src-tauri/commander-free/scripts/modules/tweaks/system.ps1").text();

    expect(system).toContain("function Get-DesktopShellPriorityStatus");
    expect(system).toContain("function Wait-DesktopShellPriorityStatus");
    expect(system).toContain("function Test-ShellPriorityTaskOwned");
    expect(system).toContain("function Test-ShellPriorityPathSecure");
    expect(system).toContain("$script:LegacyShellPriorityTaskName = 'WinCommanderShellPriorityLogon'");
    expect(system).toContain("$status = Wait-DesktopShellPriorityStatus");
    expect(system).toContain("Task Scheduler does not always return a newly registered SYSTEM task");
    expect(system).toContain("if (-not $status.enabled) { throw 'Windows did not retain the desktop-shell priority configuration.' }");
    expect(system).toContain("verified = $true");
  });
});
