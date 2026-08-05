import { describe, expect, test } from "bun:test";
import { PRIVACY_TOGGLES } from "./privacy.toggles";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

function read(path: string): Promise<string> {
  return Bun.file(path).text();
}

describe("PowerShell terminal history privacy control", () => {
  test("uses a reversible free privacy toggle", () => {
    const toggle = PRIVACY_TOGGLES.find((entry) => entry.id === "disableTerminalHistory");

    expect(toggle).toMatchObject({
      tier: "free",
      needsAdmin: false,
      settingsPath: "ideal.privacy.tracking.terminalHistoryDisabled",
      currentPath: "current.privacy.tracking.terminalHistoryDisabled",
      enableCmd: "Disable-TerminalHistory",
      disableCmd: "Enable-TerminalHistory",
      statusCmd: "Get-TerminalHistoryStatus",
    });
  });

  test("configures both PowerShell hosts and removes only its managed profile block", async () => {
    const script = await read("src-tauri/commander-free/scripts/modules/privacy/cleanup.ps1");

    expect(script).toContain("WindowsPowerShell\\profile.ps1");
    expect(script).toContain("PowerShell\\profile.ps1");
    expect(script).toContain("Set-PSReadLineOption -HistorySaveStyle SaveNothing");
    expect(script).toContain("# >>> WinCommander terminal history >>>");
    expect(script).toContain("function Enable-TerminalHistory");
    expect(script).toContain("[regex]::Replace($existing, $pattern, '')");
  });

  test("routes the action and persists its verified current state", async () => {
    const [backend, probe] = await Promise.all([
      read("src-tauri/commander-free/src/backend.rs"),
      read("src-tauri/commander-free/scripts/modules/tweaks/system.ps1"),
    ]);

    expect(backend).toContain('"Disable-TerminalHistory" | "Enable-TerminalHistory" | "Get-TerminalHistoryStatus"');
    expect(backend).toContain('"terminalHistoryDisabled": true');
    expect(backend).toContain('"terminalHistoryDisabled": false');
    expect(probe).toContain("terminalHistoryDisabled  = [bool](Get-TerminalHistoryStatus).disabled");
  });
});
