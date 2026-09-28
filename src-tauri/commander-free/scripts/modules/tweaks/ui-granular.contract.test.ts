import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const uiGranular = readFileSync(
  "src-tauri/commander-free/scripts/modules/tweaks/ui-granular.ps1",
  "utf8",
);
const settingsBridge = readFileSync(
  "src-tauri/commander-free/scripts/core/settings-bridge.ps1",
  "utf8",
);

test("desktop built-in icon toggles are idempotent and never create shortcut files", () => {
  expect(uiGranular).toContain("$alreadyApplied = $true");
  expect(uiGranular).toContain('status = if ($Visible) { "already-shown" } else { "already-hidden" }');
  expect(uiGranular).toContain("changed = $false");
  expect(uiGranular).toContain("changed = $true");
  expect(uiGranular).not.toContain("CreateShortcut");
  expect(uiGranular).not.toContain("New-Object -ComObject WScript.Shell");
});

test("desktop icon status reconciles both Windows shell locations", () => {
  expect(settingsBridge).toContain("HideDesktopIcons\\NewStartPanel");
  expect(settingsBridge).toContain("HideDesktopIcons\\ClassicStartMenu");
  expect(settingsBridge).toContain("$visible = $false");
});
