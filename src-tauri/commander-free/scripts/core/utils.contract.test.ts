import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const utils = readFileSync("src-tauri/commander-free/scripts/core/utils.ps1", "utf8");
const ui = readFileSync("src-tauri/commander-free/scripts/modules/tweaks/ui.ps1", "utf8");

test("machine-wide Explorer changes restart every active user session", () => {
  expect(utils).toContain("param([switch]$AllUsers)");
  expect(utils).toContain("Stop-Process -Force");
  expect(utils).toContain("all_users_restarted");
  expect(ui).toContain("Set-AllUserExplorerDword");
  expect(ui).toContain("Restart-Explorer -AllUsers");
});
