import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

test.skipIf(process.platform !== "win32")("OFF removes both launchers, ON recreates them, and reconciliation preserves OFF and foreign tasks", () => {
  const source = readFileSync("src-tauri/commander-free/src/autostart.rs", "utf8");
  function script(name: string) {
    const match = source.match(new RegExp(`const POWERSHELL_${name}: &str = r#"([\\s\\S]*?)"#;`));
    if (!match) throw new Error(`Missing ${name} script`);
    return match[1].replaceAll("exit 0", "return");
  }
  const common = script("COMMON").replace(/__[A-Z_]+__/g, token => ({
    __TARGET_EXE__: "(Join-Path $env:ProgramFiles 'WinCommander\\wincommander-free.exe')",
    __DESIRED_TASK_NAME__: "'SL-AS'", __ALTERNATE_TASK_NAME__: "'SL-AS'",
    __COVERED_TASK_NAME__: "'System Update Service'", __PREFERENCE_PATH__: "'unused'",
    __PREFERENCE_VALUE_NAME__: "'AutostartEnabled'", __LEGACY_DATA_DIR_NAME__: "'WinCommander'",
    __RUN_VALUE_NAMES__: "'WinCommander'",
  })[token] ?? token);
  const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", `
${common}
$script:tasks=@{}; $script:preference=1
function Get-TaskOrNull($Name) { return $script:tasks[$Name] }
function Get-AutostartPreference { return $script:preference }
function Set-AutostartPreference($Value) { $script:preference=$Value }
function Require-AutostartElevation { }
function Get-OwnedRunEntries { }
function Get-OwnedStartupShortcutEntries { }
function Test-LegacyReopenMarker { return $false }
function Remove-OwnedRunValues { }
function Remove-OwnedStartupShortcuts { }
function Remove-LegacyReopenMarker { }
function Remove-CurrentUserOwnedCompetingRoutes { }
function Unregister-ScheduledTask { param($TaskName,$Confirm,$ErrorAction) [void]$script:tasks.Remove($TaskName) }
function New-ScheduledTaskAction { param($Execute,$Argument) [pscustomobject]@{Execute=$Execute;Arguments=$Argument} }
function New-ScheduledTaskTrigger { param([switch]$AtLogOn) [pscustomobject]@{CimClass=[pscustomobject]@{CimClassName='MSFT_TaskLogonTrigger'};Enabled=$true;UserId=$null} }
function New-ScheduledTaskPrincipal { param($GroupId,$RunLevel) [pscustomobject]@{GroupId=$GroupId;RunLevel=$RunLevel} }
function New-ScheduledTaskSettingsSet { param([switch]$AllowStartIfOnBatteries,[switch]$DontStopIfGoingOnBatteries,$ExecutionTimeLimit,$MultipleInstances) [pscustomobject]@{ExecutionTimeLimit='PT0S';MultipleInstances=$MultipleInstances} }
function Register-ScheduledTask {
  param($TaskName,$Description,$Action,$Trigger,$Principal,$Settings,[switch]$Force,$ErrorAction)
  $script:tasks[$TaskName]=[pscustomobject]@{State='Ready';Actions=@($Action);Triggers=@($Trigger | Where-Object { $null -ne $_ });Principal=$Principal;Settings=$Settings}
}
function LegacyTask($Arguments,$Path=$targetExe) { [pscustomobject]@{State='Ready';Actions=@([pscustomobject]@{Execute=$Path;Arguments=$Arguments})} }
$script:tasks['WinCommander Autostart']=LegacyTask '--autostart'
$script:tasks['WinCommander Elevated Launcher']=LegacyTask '--elevated-relaunch'
$script:tasks['System Update Service']=LegacyTask '--autostart' 'C:\\Other\\app.exe'
& { ${script("DISABLE")} }
if ($script:preference -ne 0 -or $script:tasks.Count -ne 1 -or -not $script:tasks.ContainsKey('System Update Service')) { throw 'OFF retained an owned launcher or removed a foreign task' }
& { ${script("ENSURE")} }
if ($script:tasks.Count -ne 1) { throw 'Integrity repair recreated a task after OFF' }
& { ${script("ENABLE")} }
if ($script:preference -ne 1 -or $script:tasks.Count -ne 3 -or -not (Test-CanonicalTask $script:tasks['SL-AS']) -or -not (Test-CanonicalLauncher $script:tasks['SL-EL'])) { throw 'ON did not recreate the safe canonical pair' }
& { ${script("ENSURE")} }
if ($script:tasks.Count -ne 3) { throw 'Integrity repair duplicated routes' }
& { ${script("DISABLE")} }
if ($script:tasks.Count -ne 1) { throw 'OFF did not remove the newly named pair' }
$script:tasks['WinCommander Elevated Launcher']=LegacyTask '--elevated-relaunch'
& { ${script("ENSURE")} }
if ($script:tasks.Count -ne 1) { throw 'Explicit OFF did not clean a lingering legacy launcher' }
$script:tasks['SL-EL']=LegacyTask '--elevated-relaunch' 'C:\\Other\\app.exe'
$blocked=$false
try { & { ${script("ENABLE")} } } catch { $blocked=$true }
if (-not $blocked -or $script:preference -ne 0 -or $script:tasks.ContainsKey('SL-AS') -or $script:tasks['SL-EL'].Actions[0].Execute -ne 'C:\\Other\\app.exe') { throw 'Foreign canonical launcher was overwritten or failed enable created a logon route' }
Write-Output 'PASS'
`], { encoding: "utf8" });
  expect(result.stderr.trim()).toBe("");
  expect(result.status).toBe(0);
  expect(result.stdout.trim()).toBe("PASS");
});

test.skipIf(process.platform !== "win32")("installer updates preserve OFF and rename only owned startup tasks", () => {
  const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", `
$ErrorActionPreference='Stop'
$tokens=$null; $errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path (Get-Location) 'src-tauri/commander-free/nsis/configure-elevated-launchers.ps1'),[ref]$tokens,[ref]$errors)
if ($errors.Count) { throw 'Installer did not parse' }
foreach ($statement in $ast.EndBlock.Statements) {
  if ($statement -is [Management.Automation.Language.FunctionDefinitionAst] -or $statement -is [Management.Automation.Language.AssignmentStatementAst]) { Invoke-Expression $statement.Extent.Text }
}
$targetPath='C:\\Program Files\\WinCommander\\wincommander-free.exe'
$script:tasks=@{}; $script:enabled=$false
$RemoveAutostartRoutes=$false; $PreserveAutostartPreference=$true
function Get-OwnedExecutablePaths { return @($targetPath) }
function Remove-OwnedRunValues { return 0 }
function Get-AutostartEnabled { return $script:enabled }
function Get-ScheduledTask { param($TaskName,$ErrorAction) return $script:tasks[$TaskName] }
function Unregister-ScheduledTask { param($TaskName,$Confirm,$ErrorAction) [void]$script:tasks.Remove($TaskName) }
function New-ScheduledTaskAction { param($Execute,$Argument) [pscustomobject]@{Execute=$Execute;Arguments=$Argument} }
function New-ScheduledTaskTrigger { param([switch]$AtLogOn) [pscustomobject]@{CimClass=[pscustomobject]@{CimClassName='MSFT_TaskLogonTrigger'};Enabled=$true;UserId=$null} }
function New-ScheduledTaskPrincipal { param($GroupId,$RunLevel) [pscustomobject]@{GroupId=$GroupId;RunLevel=$RunLevel} }
function New-ScheduledTaskSettingsSet { param([switch]$AllowStartIfOnBatteries,[switch]$DontStopIfGoingOnBatteries,$ExecutionTimeLimit,$MultipleInstances) [pscustomobject]@{ExecutionTimeLimit='PT0S';MultipleInstances=$MultipleInstances} }
function Register-ScheduledTask {
  param($TaskName,$Description,$Action,$Trigger,$Principal,$Settings,[switch]$Force,$ErrorAction)
  $script:tasks[$TaskName]=[pscustomobject]@{State='Ready';Actions=@($Action);Triggers=@($Trigger | Where-Object { $null -ne $_ });Principal=$Principal;Settings=$Settings}
}
function LegacyTask($Arguments,$Path=$targetPath) { [pscustomobject]@{State='Ready';Actions=@([pscustomobject]@{Execute=$Path;Arguments=$Arguments})} }
$script:tasks['WinCommander Autostart']=LegacyTask '--autostart'
$script:tasks['WinCommander Elevated Launcher']=LegacyTask '--elevated-relaunch'
$script:tasks['Sys Health Checker']=LegacyTask '--autostart' 'C:\\Other\\app.exe'
$script:tasks['WinCommander Input Service']=LegacyTask '--minimized'
$script:tasks['System Update Service']=LegacyTask ("-Command & '" + $targetPath + "' --autostart 2>autostart.stderr.log") 'powershell.exe'
$run=[scriptblock]::Create($ast.EndBlock.Statements[-1].Extent.Text)
& $run | Out-Null
if ($script:tasks.Count -ne 1 -or -not $script:tasks.ContainsKey('Sys Health Checker')) { throw 'Installer restored launcher despite OFF or removed foreign task' }
$script:enabled=$true
$script:tasks['WinCommander Autostart']=LegacyTask '--autostart'
$script:tasks['WinCommander Elevated Launcher']=LegacyTask '--elevated-relaunch'
& $run | Out-Null
if ($script:tasks.Count -ne 3 -or -not $script:tasks.ContainsKey('SL-AS') -or -not $script:tasks.ContainsKey('SL-EL')) { throw 'Installer did not migrate to compact task names' }
$script:enabled=$false
& $run | Out-Null
if ($script:tasks.Count -ne 1) { throw 'Update retained canonical elevated launcher with OFF' }
$script:enabled=$true
$script:tasks['SL-AS']=LegacyTask '--autostart' 'C:\\Other\\app.exe'
$blocked=$false
try { & $run | Out-Null } catch { $blocked=$true }
if (-not $blocked -or $script:tasks.ContainsKey('SL-EL') -or $script:tasks['SL-AS'].Actions[0].Execute -ne 'C:\\Other\\app.exe') { throw 'Installer name conflict partially registered a launcher or changed the foreign task' }
Write-Output 'PASS'
`], { encoding: "utf8" });
  expect(result.stderr.trim()).toBe("");
  expect(result.status).toBe(0);
  expect(result.stdout.trim()).toBe("PASS");
});
