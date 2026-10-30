import { expect, test } from "bun:test";

test("account browser summaries load the same browser inventory dependency as local scans", async () => {
  const backend = await Bun.file("src-tauri/commander-free/src/backend.rs").text();
  const condition = backend.indexOf('} else if command == "Get-BrowserFootprints"');
  const moduleLoad = backend.indexOf('additional_modules.push_str(&load_module("tweaks/security")?)', condition);
  expect(condition).toBeGreaterThanOrEqual(0);
  expect(moduleLoad).toBeGreaterThan(condition);
  expect(backend.slice(condition, moduleLoad)).toContain('command == "Get-CleanupSummaryAllUsers"');
});
