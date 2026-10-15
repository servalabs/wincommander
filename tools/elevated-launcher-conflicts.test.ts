import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";

function runFixture(mode: string, defer = true, uninstall = false) {
  return spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", String.raw`
$ErrorActionPreference='Stop'
Import-Module ScheduledTasks -ErrorAction Stop
$mode='${mode}'; $DeferAutostartConflict=$${defer}; $RemoveAutostartRoutes=$${uninstall}
$RemoveManualLauncher=$false; $RemoveAutostartPreference=$false; $PreserveAutostartPreference=$true
$startupMutationStarted=$false
$targetPath='C:\Program Files\WinCommander\wincommander-free.exe'
$manualTaskName='SM-EL'; $autostartTaskName='SM-AS'; $systemMaintenanceTaskPath='\System Maintenance\'
$tokens=$null; $errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path (Get-Location) 'src-tauri/commander-free/nsis/configure-elevated-launchers.ps1'),[ref]$tokens,[ref]$errors)
if ($errors.Count) { throw 'Invalid launcher source' }
foreach($name in @('Test-OwnedExecutablePath','Test-OwnedExecutableCommand','Test-TaskActionOwnership','Assert-TaskNameCanBeReconciled')) {
  $definition=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name}.GetNewClosure(),$true)
  if ($null -eq $definition) { throw 'Missing ownership function' }
  Invoke-Expression $definition.Extent.Text
}
$script:queries=0; $script:preference=0
function Get-OwnedExecutablePaths { return @($targetPath) }
function Get-ScheduledTask {
  [CmdletBinding()] param([string]$TaskPath,[string]$TaskName)
  $script:queries++
  if ($mode -eq 'missing_task') {
    return ScheduledTasks\Get-ScheduledTask -TaskPath '\' -TaskName ('WinCommander_ReadonlyMissing_'+[guid]::NewGuid().ToString('N')) -ErrorAction Stop
  }
  if ($mode -eq 'missing_folder') {
    return ScheduledTasks\Get-ScheduledTask -TaskPath ('\WinCommander_ReadonlyMissing_'+[guid]::NewGuid().ToString('N')+'\') -TaskName $TaskName -ErrorAction Stop
  }
  if ($mode -eq 'query_denied') { throw [UnauthorizedAccessException]::new('denied') }
  if ($mode -eq 'query_unknown') { throw 'Unknown scheduler query failure' }
  $arguments=if($TaskName -eq 'SM-EL'){'--elevated-relaunch $(Arg0)'}else{'--autostart'}
  $execute=$targetPath
  if ($TaskName -eq 'SM-AS' -and $mode -in @('foreign','lookalike','multiple','race')) {
    if ($mode -ne 'race' -or $script:queries -gt 2) {
      $execute=if($mode -eq 'lookalike'){'D:\UntrustedCheckout\wincommander-free.exe'}else{'C:\Other\other.exe'}
    }
  }
  $actions=@([pscustomobject]@{Execute=$execute;Arguments=$arguments})
  if($mode -eq 'multiple' -and $TaskName -eq 'SM-AS') { $actions+=$actions[0] }
  return [pscustomobject]@{Actions=$actions}
}
function Get-AutostartEnabled { param($preserve,$paths); if($mode -eq 'off'){ return $false }; Write-Host 'MUTATION:preference'; return $true }
function Remove-OwnedRunValues { param($paths,$owned); Write-Host 'MUTATION:run'; return 0 }
function Register-ElevatedLauncherTask {
  if ($mode -eq 'security_failure') { throw 'Task contract readback failed' }
  Write-Host 'MUTATION:manual'
}
function Register-LogonRouterTask {
  Assert-TaskNameCanBeReconciled $autostartTaskName '--autostart' @($targetPath) $systemMaintenanceTaskPath
  Write-Host 'MUTATION:router'
}
function Remove-OtherAutostartTasks { param($owned); Write-Host 'MUTATION:legacy'; return 0 }
function Remove-AllAutostartTasks { param($owned); if($mode -eq 'remove_failure'){throw 'Removal failed'}; Write-Host 'MUTATION:ownedcleanup'; return 0 }
function Remove-OwnedNamedTask { throw 'Unexpected manual removal' }
function Remove-AutostartPreference { throw 'Unexpected preference removal' }
$main=@($ast.EndBlock.Statements | Where-Object {$_ -is [Management.Automation.Language.TryStatementAst]})[-1]
& ([scriptblock]::Create($main.Extent.Text))
if($script:preference -ne 0) { throw 'Explicit OFF changed' }
'FIXTURE_COMPLETED'
  `], { encoding: "utf8", windowsHide: true });
}

test.skipIf(process.platform !== "win32")("only opted-in preflight conflicts defer without changing tasks, Run values or preferences", () => {
  for (const mode of ["foreign", "lookalike", "multiple"]) {
    const deferred = runFixture(mode);
    expect(deferred.status, deferred.stdout + deferred.stderr).toBe(3);
    expect(deferred.stdout).toContain("Existing tasks and preferences were kept");
    expect(deferred.stdout).not.toContain("MUTATION:");
    const strict = runFixture(mode, false);
    expect(strict.status).toBe(1);
    expect(strict.stdout).not.toContain("MUTATION:");
  }
}, 20_000);

test.skipIf(process.platform !== "win32")("unknown Scheduler state is fatal and never grants replacement authority", () => {
  for (const mode of ["query_denied", "query_unknown"]) {
    const result = runFixture(mode);
    expect(result.status, result.stdout + result.stderr).toBe(1);
    expect(result.stdout).not.toContain("MUTATION:");
    expect(result.stderr.replace(/\s+/g, " ")).toContain("could not verify the existing startup task");
  }
}, 20_000);

test.skipIf(process.platform !== "win32")("real read-only missing-task and missing-folder queries permit first installation", () => {
  for (const mode of ["missing_task", "missing_folder"]) {
    const result = runFixture(mode);
    expect(result.status, result.stdout + result.stderr).toBe(0);
    expect(result.stdout).toContain("FIXTURE_COMPLETED");
    expect(result.stdout).toContain("MUTATION:router"); // Mocked calls only.
  }
}, 20_000);

test.skipIf(process.platform !== "win32")("owned routes and explicit OFF retain their existing behavior", () => {
  const owned = runFixture("owned");
  expect(owned.status, owned.stdout + owned.stderr).toBe(0);
  expect(owned.stdout).toContain("MUTATION:router");
  const off = runFixture("off");
  expect(off.status, off.stdout + off.stderr).toBe(0);
  expect(off.stdout).toContain("MUTATION:ownedcleanup");
  expect(off.stdout).not.toContain("MUTATION:preference");
  expect(off.stdout).not.toContain("MUTATION:router");
}, 20_000);

test.skipIf(process.platform !== "win32")("security failures, post-preflight races and uninstall errors remain fatal", () => {
  for (const mode of ["security_failure", "race"]) {
    const result = runFixture(mode);
    expect(result.status, result.stdout + result.stderr).toBe(1);
    expect(result.stdout).toContain("MUTATION:");
    expect(result.stdout).not.toContain("Existing tasks and preferences were kept");
  }
  const uninstall = runFixture("remove_failure", true, true);
  expect(uninstall.status).toBe(1);
  expect(uninstall.stdout).not.toContain("Existing tasks and preferences were kept");
}, 20_000);

test("NSIS defers only the exact opted-in conflict result", () => {
  const source = readFileSync("src-tauri/commander-free/nsis/hooks.nsh", "utf8");
  const configureStart = source.indexOf("!macro WC_CONFIGURE_ELEVATED_LAUNCHERS");
  const uninstallStart = source.indexOf("!macro WC_REMOVE_AUTOSTART_ROUTES_OR_ABORT");
  expect(configureStart).toBeGreaterThanOrEqual(0);
  expect(uninstallStart).toBeGreaterThan(configureStart);
  const configure = source.slice(configureStart, uninstallStart);
  expect(configure).toContain("-DeferAutostartConflict");
  expect(configure).toContain("${If} $0 == 3");
  expect(configure).toContain("${ElseIf} $0 != 0");
  expect(configure).toContain("Abort ");
  const uninstall = source.slice(uninstallStart);
  expect(uninstall).not.toContain("-DeferAutostartConflict");
});
