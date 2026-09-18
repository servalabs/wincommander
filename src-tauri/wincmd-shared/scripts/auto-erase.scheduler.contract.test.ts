import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const scheduler = readFileSync("src-tauri/wincmd-shared/scripts/auto-erase.ps1", "utf8");
const categories = readFileSync("src/panels/cleanup/cleanupCategories.ts", "utf8");

test("the scheduler exposes only a real, SYSTEM-scoped firewall-log clearer", () => {
  expect(scheduler).toContain("'firewallLog'");
  expect(scheduler).toContain("Get-NetFirewallProfile");
  expect(scheduler).toContain("Erase-OneFile `$path");
  expect(categories).toContain("'firewallLog'");
  expect(categories).toContain("'eventLogs', 'searchIndex', 'amcache', 'recallDb', 'branchCache'");
});

test("bulk scheduling can preserve a manually configured scheduled task", () => {
  expect(scheduler).toContain("[switch]$PreserveExisting");
  expect(scheduler).toContain("Get-ScheduledTask -TaskName $taskName");
  expect(scheduler).toContain("status          = 'alreadyConfigured'");
  expect(scheduler).toContain("per-card editor intentionally leaves it off");
});

test("missed intervals catch up once at sign-in or startup and only success advances the marker", () => {
  expect(scheduler).toContain("state.json");
  expect(scheduler).toContain("lastSuccessfulUtc");
  expect(scheduler).toContain("nextDueUtc");
  expect(scheduler).toContain("`$success = (`$script:AutoEraseFailed -eq 0)");
  expect(scheduler).toContain("New-ScheduledTaskTrigger -AtStartup");
  expect(scheduler).toContain("New-ScheduledTaskTrigger -AtLogOn -User $TargetUser");
  expect(scheduler).toContain("-At (Get-Date).AddMinutes($IntervalMinutes)");
  expect(scheduler).toContain("scheduled wipe v2");
  expect(scheduler).toContain("catch-up marker behind");
});

test("unsafe and one-shot cleanup operations are not scheduler payloads", () => {
  for (const categoryId of [
    "shadowCopies", "wslData", "dockerDesktopData", "virtualMachineArtifacts",
    "credentialManager", "virtualMemory", "unallocatedErase", "previousWindowsInstall",
  ]) {
    expect(scheduler).not.toContain(`'${categoryId}'`);
  }
});
