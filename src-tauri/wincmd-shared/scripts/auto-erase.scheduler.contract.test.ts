import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { ALL_CATEGORIES } from "../../../src/panels/cleanup/cleanupCategories";

const scheduler = readFileSync("src-tauri/wincmd-shared/scripts/auto-erase.ps1", "utf8");
const categories = readFileSync("src/panels/cleanup/cleanupCategories.ts", "utf8");

test("the scheduler exposes only a real, SYSTEM-scoped firewall-log clearer", () => {
  expect(scheduler).toContain("'firewallLog'");
  expect(scheduler).toContain("Get-NetFirewallProfile");
  expect(scheduler).toContain("Erase-OneFile `$path");
  expect(categories).toContain("'firewallLog'");
  expect(ALL_CATEGORIES.find(category => category.id === "firewallLog")?.schedulerRunAsSystem).toBe(true);
});

test("bulk scheduling can preserve a manually configured scheduled task", () => {
  expect(scheduler).toContain("[switch]$PreserveExisting");
  expect(scheduler).toContain("Get-ScheduledTask -TaskPath $script:SystemMaintenanceTaskPath -TaskName $taskName");
  expect(scheduler).toContain("if ($PreserveExisting -and $existing)");
  expect(scheduler).toContain("status          = 'alreadyConfigured'");
  expect(scheduler).toContain("per-card editor intentionally leaves it off");
});

test("missed intervals catch up once at sign-in or startup and only success advances the marker", () => {
  expect(scheduler).toContain("state.json");
  expect(scheduler).toContain("lastSuccessfulUtc");
  expect(scheduler).toContain("nextDueUtc");
  expect(scheduler).toContain("$success = ($Failed -eq 0 -and $Deferred -eq 0 -and -not $CoverageIncomplete)");
  expect(scheduler).toContain("New-ScheduledTaskTrigger -AtStartup");
  expect(scheduler).toContain("New-ScheduledTaskTrigger -AtLogOn -User $TargetUser");
  expect(scheduler).toContain("-At ($firstDue.ToLocalTime())");
  expect(scheduler).toContain("if ($success) { $LastSuccess = $completed }");
  expect(scheduler).toContain("catch-up marker behind");
});

test("one-shot cleanup operations stay unscheduled and sensitive recurring cleanup requires individual consent", () => {
  for (const categoryId of [
    "virtualMemory", "unallocatedErase", "previousWindowsInstall",
  ]) {
    expect(scheduler).not.toContain(`'${categoryId}'`);
  }
  expect(scheduler).toContain("$ManagedByAutoSet -and $CategoryId -in $script:AutoEraseManualOnlyCategories");
  expect(scheduler).toContain("$CategoryId -in $script:AutoEraseProtectedCategories -and ($ManagedByAutoSet -or -not $AllowDefaultExcluded)");
});
