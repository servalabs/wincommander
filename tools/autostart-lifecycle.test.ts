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
    __DESIRED_TASK_NAME__: "'SM-AS'", __ALTERNATE_TASK_NAME__: "'SM-AS'",
    __TASK_PATH__: "'\\System Maintenance\\'", __PREFERENCE_PATH__: "'unused'",
    __PREFERENCE_VALUE_NAME__: "'AutostartEnabled'", __LEGACY_DATA_DIR_NAME__: "'WinCommander'",
    __RUN_VALUE_NAMES__: "'WinCommander'",
  })[token] ?? token);
  const input = `
${common}
$script:tasks=@{}; $script:preference=1
function Get-TaskOrNull($Name) { return $script:tasks[$Name] }
function Ensure-SystemMaintenanceTaskFolder { }
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
function Unregister-ScheduledTask { param($TaskPath,$TaskName,$Confirm,$ErrorAction) [void]$script:tasks.Remove($TaskName) }
function New-ScheduledTaskAction { param($Execute,$Argument) [pscustomobject]@{Execute=$Execute;Arguments=$Argument} }
function New-ScheduledTaskTrigger { param([switch]$AtLogOn) [pscustomobject]@{CimClass=[pscustomobject]@{CimClassName='MSFT_TaskLogonTrigger'};Enabled=$true;UserId=$null;Repetition=[pscustomobject]@{Interval=''}} }
function New-ScheduledTaskPrincipal { param($GroupId,$RunLevel) [pscustomobject]@{GroupId=$GroupId;RunLevel=$RunLevel} }
function New-ScheduledTaskSettingsSet { param([switch]$AllowStartIfOnBatteries,[switch]$DontStopIfGoingOnBatteries,$ExecutionTimeLimit,$MultipleInstances) [pscustomobject]@{ExecutionTimeLimit='PT0S';MultipleInstances=$MultipleInstances;AllowDemandStart=$true;RestartCount=0;StartWhenAvailable=$false;WakeToRun=$false} }
function Register-ScheduledTask {
  param($TaskPath,$TaskName,$Description,$Action,$Trigger,$Principal,$Settings,[switch]$Force,$ErrorAction)
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
if ($script:preference -ne 1 -or $script:tasks.Count -ne 3 -or -not (Test-CanonicalTask $script:tasks['SM-AS']) -or -not (Test-CanonicalLauncher $script:tasks['SM-EL'])) { throw 'ON did not recreate the safe canonical pair' }
if ($script:tasks['SM-AS'].Principal.GroupId -ne 'S-1-5-32-545' -or $script:tasks['SM-AS'].Principal.RunLevel -ne 'Highest') { throw 'Installed startup must use each signing-in Users member with their highest available token' }
$script:tasks['SM-AS'].Principal.RunLevel='Limited'
if (Test-CanonicalTask $script:tasks['SM-AS']) { throw 'Legacy limited startup was incorrectly accepted as the installed contract' }
& { ${script("ENSURE")} }
if ($script:tasks['SM-AS'].Principal.RunLevel -ne 'Highest' -or $script:tasks.Count -ne 3) { throw 'Reconciliation did not replace the limited route without adding a trigger' }
& { ${script("ENSURE")} }
if ($script:tasks.Count -ne 3) { throw 'Integrity repair duplicated routes' }
$script:tasks['SM-EL'].Triggers=$null
if (-not (Test-CanonicalLauncher $script:tasks['SM-EL'])) { throw 'Windows CIM null triggers falsely required launcher repair' }
$script:tasks['SM-AS'].Triggers[0].Repetition=$null
if (-not (Test-CanonicalTask $script:tasks['SM-AS'])) { throw 'A non-repeating Windows trigger was rejected' }
$script:tasks['SM-AS'].Triggers[0].Repetition=[pscustomobject]@{Interval=''}
function Read-EffectiveStatus { ${script("STATUS").replace("[Console]::Out.Write(([bool]$enabled).ToString().ToLowerInvariant())", "return [bool]$enabled")} }
$launcher=$script:tasks['SM-EL']; [void]$script:tasks.Remove('SM-EL')
if (-not (Read-EffectiveStatus)) { throw 'A missing manual launcher falsely reported a live sign-in route as OFF' }
$script:preference=0
if (-not (Read-EffectiveStatus)) { throw 'An interrupted OFF operation hid the remaining live route' }
$script:preference=1; $script:tasks['SM-EL']=$launcher
$script:tasks['SM-AS'].Settings.RestartCount=3
if (Test-CanonicalTask $script:tasks['SM-AS']) { throw 'An automatic restart loop was accepted as healthy' }
& { ${script("ENSURE")} }
if ($script:tasks['SM-AS'].Settings.RestartCount -ne 0) { throw 'Reconciliation did not remove automatic restarts' }
$script:tasks['SM-AS'].Triggers[0].Repetition.Interval='PT1M'
if (Test-CanonicalTask $script:tasks['SM-AS']) { throw 'A repeating logon trigger was accepted as healthy' }
& { ${script("ENSURE")} }
if ($script:tasks['SM-AS'].Triggers[0].Repetition.Interval) { throw 'Reconciliation retained a repeating logon trigger' }
$script:tasks['SM-EL'].Triggers=@(New-ScheduledTaskTrigger -AtLogOn)
if (Test-CanonicalLauncher $script:tasks['SM-EL']) { throw 'The manual launcher accepted an automatic trigger' }
& { ${script("ENSURE")} }
if ($script:tasks['SM-EL'].Triggers.Count -ne 0) { throw 'Reconciliation retained a second logon trigger' }
& { ${script("DISABLE")} }
if ($script:tasks.Count -ne 1) { throw 'OFF did not remove the newly named pair' }
if (Read-EffectiveStatus) { throw 'Removed startup routes were still reported ON' }
$script:tasks['SM-EL']=$launcher
$script:tasks['SM-EL'].Triggers=$null
if (Read-EffectiveStatus) { throw 'Manual launcher with CIM null triggers was reported as an automatic route' }
[void]$script:tasks.Remove('SM-EL')
$script:tasks['WinCommander Elevated Launcher']=LegacyTask '--elevated-relaunch'
& { ${script("ENSURE")} }
if ($script:tasks.Count -ne 1) { throw 'Explicit OFF did not clean a lingering legacy launcher' }
$script:tasks['SM-EL']=LegacyTask '--elevated-relaunch' 'C:\\Other\\app.exe'
$blocked=$false
try { & { ${script("ENABLE")} } } catch { $blocked=$true }
if (-not $blocked -or $script:preference -ne 0 -or $script:tasks.ContainsKey('SM-AS') -or $script:tasks['SM-EL'].Actions[0].Execute -ne 'C:\\Other\\app.exe') { throw 'Foreign canonical launcher was overwritten or failed enable created a logon route' }
$targetExe='C:\\Portable\\WinCommander\\wincommander-free.exe'
$ownedExePaths=@($targetExe); $script:tasks=@{}; $script:preference=0
& { ${script("ENABLE")} }
if ($script:tasks.Count -ne 1 -or $script:tasks['SM-AS'].Principal.RunLevel -ne 'Limited' -or -not (Test-CanonicalTask $script:tasks['SM-AS'])) { throw 'Portable startup acquired persistent elevated privileges' }
$script:tasks['SM-AS'].Principal.RunLevel='Highest'
if (Test-CanonicalTask $script:tasks['SM-AS']) { throw 'Portable elevated startup was incorrectly accepted as healthy' }
& { ${script("ENSURE")} }
if ($script:tasks['SM-AS'].Principal.RunLevel -ne 'Limited') { throw 'Portable startup retained a persistent elevated task' }
Write-Output 'PASS'
`;
  const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", "$code=[Console]::In.ReadToEnd(); & ([scriptblock]::Create($code))"], { input, encoding: "utf8", windowsHide: true });
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
function Ensure-SystemMaintenanceTaskFolder { }
function Get-ScheduledTask { param($TaskPath,$TaskName,$ErrorAction) return $script:tasks[$TaskName] }
function Unregister-ScheduledTask { param($TaskPath,$TaskName,$Confirm,$ErrorAction) [void]$script:tasks.Remove($TaskName) }
function New-ScheduledTaskAction { param($Execute,$Argument) [pscustomobject]@{Execute=$Execute;Arguments=$Argument} }
function New-ScheduledTaskTrigger { param([switch]$AtLogOn) [pscustomobject]@{CimClass=[pscustomobject]@{CimClassName='MSFT_TaskLogonTrigger'};Enabled=$true;UserId=$null;Repetition=[pscustomobject]@{Interval=''}} }
function New-ScheduledTaskPrincipal { param($GroupId,$RunLevel) [pscustomobject]@{GroupId=$GroupId;RunLevel=$RunLevel} }
function New-ScheduledTaskSettingsSet { param([switch]$AllowStartIfOnBatteries,[switch]$DontStopIfGoingOnBatteries,$ExecutionTimeLimit,$MultipleInstances) [pscustomobject]@{ExecutionTimeLimit='PT0S';MultipleInstances=$MultipleInstances;AllowDemandStart=$true;RestartCount=0;StartWhenAvailable=$false;WakeToRun=$false} }
function Register-ScheduledTask {
  param($TaskPath,$TaskName,$Description,$Action,$Trigger,$Principal,$Settings,[switch]$Force,$ErrorAction)
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
$script:tasks['SL-AS']=LegacyTask '--autostart'
$script:tasks['SL-EL']=LegacyTask '--elevated-relaunch $(Arg0)'
& $run | Out-Null
if ($script:tasks.Count -ne 3 -or -not $script:tasks.ContainsKey('SM-AS') -or -not $script:tasks.ContainsKey('SM-EL')) { throw 'Installer did not migrate to compact task names' }
if ($script:tasks['SM-AS'].Principal.GroupId -ne $usersSid -or $script:tasks['SM-AS'].Principal.RunLevel -ne 'Highest' -or $script:tasks['SM-AS'].Actions[0].Arguments -ne '--autostart') { throw 'Installer did not register direct highest-available startup for all Users members' }
$manual=$script:tasks['SM-EL']; $manual.Triggers=@(New-ScheduledTaskTrigger -AtLogOn)
$blocked=$false
try { Assert-TaskContract $manual $administratorsSid 'Highest' '--elevated-relaunch $(Arg0)' } catch { $blocked=$true }
if (-not $blocked) { throw 'Installer accepted a second automatic launch trigger' }
$manual.Triggers=@(); $manual.Settings.StartWhenAvailable=$true
$blocked=$false
try { Assert-TaskContract $manual $administratorsSid 'Highest' '--elevated-relaunch $(Arg0)' } catch { $blocked=$true }
if (-not $blocked) { throw 'Installer accepted catch-up relaunches' }
$script:enabled=$false
& $run | Out-Null
if ($script:tasks.Count -ne 1) { throw 'Update retained canonical elevated launcher with OFF' }
$script:enabled=$true
$script:tasks['SM-AS']=LegacyTask '--autostart' 'C:\\Other\\app.exe'
$blocked=$false
try { & $run | Out-Null } catch { $blocked=$true }
if (-not $blocked -or $script:tasks.ContainsKey('SM-EL') -or $script:tasks['SM-AS'].Actions[0].Execute -ne 'C:\\Other\\app.exe') { throw 'Installer name conflict partially registered a launcher or changed the foreign task' }
Write-Output 'PASS'
`], { encoding: "utf8" });
  expect(result.stderr.trim()).toBe("");
  expect(result.status).toBe(0);
  expect(result.stdout.trim()).toBe("PASS");
});
