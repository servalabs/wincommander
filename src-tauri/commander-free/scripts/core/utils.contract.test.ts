import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const utils = readFileSync("src-tauri/commander-free/scripts/core/utils.ps1", "utf8");
const ui = readFileSync("src-tauri/commander-free/scripts/modules/tweaks/ui.ps1", "utf8");
const autoErase = readFileSync("src-tauri/wincmd-shared/scripts/auto-erase.ps1", "utf8");

test("machine-wide Explorer changes never terminate interactive shells", () => {
  expect(utils).toContain("param([switch]$AllUsers)");
  expect(utils).toContain("SHChangeNotify");
  expect(utils).toContain("SendMessageTimeout");
  expect(utils).not.toContain("Stop-Process -Force");
  expect(utils).toContain("Server Core|ServerCore");
  expect(utils).toContain("Existing RDS and console sessions apply them when each user signs out and back in.");
  expect(ui).toContain("Set-AllUserExplorerDword");
  expect(ui).toContain("Test-ExplorerShellAvailable");
  expect(ui).toContain("unavailable on Windows Server Core");
  expect(ui).toContain("Taskbar Debloat requires Explorer Desktop Experience");
  expect(ui).toContain("Restart-Explorer -AllUsers");
  expect(autoErase).not.toContain("Stop-Process -Name explorer");
  expect(autoErase).not.toContain("Start-Process explorer.exe");
});
