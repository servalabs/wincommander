import { describe, expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";

describe("installer task readback", () => {
  test.skipIf(process.platform !== "win32")("retries after leftover offline hive mounts and preserves real cleanup failures", () => {
    const result = spawnSync("powershell.exe", [
      "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
      "-File", "tools/test-legacy-profile-hives.ps1",
    ], { encoding: "utf8" });
    expect(result.stderr.trim()).toBe("");
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("PASS: repeated setup defers offline profiles with leftover temporary hives");
    expect(result.stdout).toContain("PASS: loaded profile takes precedence over leftover temporary hive");
    expect(result.stdout).toContain("PASS: denied access is deferred; genuine cleanup failures remain errors");
  });

  test.skipIf(process.platform !== "win32")("normalizes group names and rejects the wrong privilege, executable, arguments and session policy", () => {
    const script = `
$ErrorActionPreference='Stop'
$tokens=$null; $parseErrors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path (Get-Location) 'src-tauri/commander-free/nsis/configure-elevated-launchers.ps1'),[ref]$tokens,[ref]$parseErrors)
if ($parseErrors.Count) { throw 'Invalid installer script' }
$function=$ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Assert-TaskContract' },$true)
Invoke-Expression $function.Extent.Text
$targetPath='C:\\Program Files\\WinCommander\\wincommander-free.exe'
$group=([Security.Principal.SecurityIdentifier]'S-1-5-32-544').Translate([Security.Principal.NTAccount]).Value
function Fixture {
  [pscustomobject]@{State='Ready';Triggers=@();Principal=[pscustomobject]@{GroupId=$group;RunLevel='Highest'};Settings=[pscustomobject]@{MultipleInstances='Parallel';ExecutionTimeLimit='PT0S';AllowDemandStart=$true;RestartCount=0;StartWhenAvailable=$false;WakeToRun=$false};Actions=@([pscustomobject]@{Execute=$targetPath;Arguments='--elevated-relaunch'})}
}
Assert-TaskContract (Fixture) 'S-1-5-32-544' 'Highest' '--elevated-relaunch'
$sidFixture=Fixture; $sidFixture.Principal.GroupId='S-1-5-32-544'
Assert-TaskContract $sidFixture 'S-1-5-32-544' 'Highest' '--elevated-relaunch'
$nullTriggerFixture=Fixture; $nullTriggerFixture.Triggers=$null
Assert-TaskContract $nullTriggerFixture 'S-1-5-32-544' 'Highest' '--elevated-relaunch'
foreach ($case in @('disabled','group','level','instances','time','path','arguments','trigger','restart','catchup','wake','demand')) {
  $t=Fixture
  switch ($case) {
    disabled { $t.State='Disabled' }
    group { $t.Principal.GroupId='S-1-5-32-545' }
    level { $t.Principal.RunLevel='Limited' }
    instances { $t.Settings.MultipleInstances='IgnoreNew' }
    time { $t.Settings.ExecutionTimeLimit='PT1M' }
    path { $t.Actions[0].Execute='C:\\wrong.exe' }
    arguments { $t.Actions[0].Arguments='--wrong' }
    trigger { $t.Triggers=@([pscustomobject]@{Enabled=$true}) }
    restart { $t.Settings.RestartCount=3 }
    catchup { $t.Settings.StartWhenAvailable=$true }
    wake { $t.Settings.WakeToRun=$true }
    demand { $t.Settings.AllowDemandStart=$false }
  }
  $denied=$false
  try { Assert-TaskContract $t 'S-1-5-32-544' 'Highest' '--elevated-relaunch' } catch { $denied=$true }
  if (-not $denied) { throw "Invalid task accepted: $case" }
}
Write-Output 'PASS'
`;
    const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], { encoding: "utf8" });
    expect(result.stderr.trim()).toBe("");
    expect(result.status).toBe(0);
    expect(result.stdout.trim()).toBe("PASS");
  });

  test.skipIf(process.platform !== "win32")("logon router accepts every user and rejects disabled or account-specific triggers", () => {
    const runtimeRepair = readFileSync("src-tauri/commander-free/src/autostart.rs", "utf8");
    const runtimePredicate = runtimeRepair.match(
      /^  \$triggers = .+\r?\n  \$logonTriggers = .+\r?\n  \$allUsersLogon = .+$/m,
    )?.[0] ?? runtimeRepair.match(/^  \$allUsersLogon = .+$/m)?.[0];
    expect(runtimePredicate).toBeDefined();
    const script = `
$ErrorActionPreference='Stop'
$tokens=$null; $parseErrors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path (Get-Location) 'src-tauri/commander-free/nsis/configure-elevated-launchers.ps1'),[ref]$tokens,[ref]$parseErrors)
$function=$ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Assert-TaskContract' },$true)
Invoke-Expression $function.Extent.Text
function Assert-RuntimeRouter($task) {
  $triggers=@($task.Triggers)
  $logonTrigger=@($task.Triggers | Where-Object { $_.CimClass.CimClassName -eq 'MSFT_TaskLogonTrigger' })
  $logonTriggers=@($task.Triggers | Where-Object { $_.CimClass.CimClassName -eq 'MSFT_TaskLogonTrigger' })
${runtimePredicate}
  if (-not $allUsersLogon) { throw 'Runtime repair required' }
}
$targetPath='C:\\Program Files\\WinCommander\\wincommander-free.exe'
function Fixture {
  [pscustomobject]@{State='Ready';Triggers=@([pscustomobject]@{CimClass=[pscustomobject]@{CimClassName='MSFT_TaskLogonTrigger'};Enabled=$true;UserId=$null;Repetition=[pscustomobject]@{Interval=''}});Principal=[pscustomobject]@{GroupId='S-1-5-32-545';RunLevel='Limited'};Settings=[pscustomobject]@{MultipleInstances='Parallel';ExecutionTimeLimit='PT0S';AllowDemandStart=$true;RestartCount=0;StartWhenAvailable=$false;WakeToRun=$false};Actions=@([pscustomobject]@{Execute=$targetPath;Arguments='--autostart'})}
}
Assert-TaskContract (Fixture) 'S-1-5-32-545' 'Limited' '--autostart' $true
Assert-RuntimeRouter (Fixture)
foreach ($case in @('account','disabled','extra','wrong','missing')) {
  $t=Fixture
  switch ($case) {
    account { $t.Triggers[0].UserId='S-1-5-21-100-200-300-1001' }
    disabled { $t.Triggers[0].Enabled=$false }
    extra { $t.Triggers+= $t.Triggers[0] }
    wrong { $t.Triggers[0].CimClass.CimClassName='MSFT_TaskTimeTrigger' }
    missing { $t.Triggers=@() }
  }
  $denied=$false
  try { Assert-TaskContract $t 'S-1-5-32-545' 'Limited' '--autostart' $true } catch { $denied=$true }
  if (-not $denied) { throw "Invalid logon trigger accepted: $case" }
  $denied=$false
  try { Assert-RuntimeRouter $t } catch { $denied=$true }
  if (-not $denied) { throw "Runtime repair skipped invalid logon trigger: $case" }
}
Write-Output 'PASS'
`;
    const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], { encoding: "utf8" });
    expect(result.stderr.trim()).toBe("");
    expect(result.status).toBe(0);
    expect(result.stdout.trim()).toBe("PASS");
  });

  test.skipIf(process.platform !== "win32")("treats missing Run values as clean, migrates covered opt-out, and skips foreign cleanup", () => {
    const script = `
$ErrorActionPreference='Stop'
$tokens=$null; $parseErrors=$null
$scriptPath=Join-Path (Get-Location) 'src-tauri/commander-free/nsis/configure-elevated-launchers.ps1'
$ast=[Management.Automation.Language.Parser]::ParseFile($scriptPath,[ref]$tokens,[ref]$parseErrors)
if ($parseErrors.Count) { throw 'Invalid launcher installer script' }
foreach ($functionName in @('Get-OptionalRegistryValue','Test-OwnedExecutablePath','Test-OwnedExecutableCommand','Remove-OwnedRunValues','Set-AutostartPreference','Get-AutostartEnabled','Test-TaskActionOwnership','Remove-OwnedNamedTask')) {
  $finder={ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $functionName }.GetNewClosure()
  $definition=$ast.Find($finder,$true)
  if ($null -eq $definition) { throw "Missing installer function: $functionName" }
  Invoke-Expression $definition.Extent.Text
}
$runValueNames=@('WinCommander','WinCommander Free')
$autostartTaskName='WinCommander Autostart'
$genericAutostartTaskNames=@('System Update Service','Sys Health Checker','WinCommander Input Service')
$script:scheduledTasks=@{}
$script:unregisteredTasks=@()
function Get-ScheduledTask {
  [CmdletBinding()]
  param([string]$TaskPath, [string]$TaskName)
  return $script:scheduledTasks[$TaskName]
}
function Unregister-ScheduledTask {
  [CmdletBinding(SupportsShouldProcess=$true)]
  param([string]$TaskPath, [string]$TaskName)
  $script:unregisteredTasks += $TaskName
  [void]$script:scheduledTasks.Remove($TaskName)
}
$target='C:\\Program Files\\WinCommander\\wincommander-free.exe'
$root="HKCU:\\Software\\ServaLabs\\WinCommander\\InstallerTests\\$([guid]::NewGuid().ToString('N'))"
New-Item -Path $root -Force | Out-Null
try {
  if ($null -ne (Get-OptionalRegistryValue $root 'WinCommander')) { throw 'Missing value was not empty.' }
  if ((Remove-OwnedRunValues @($root) @($target)) -ne 0) { throw 'Missing value was treated as an owned entry.' }
  $preferencePath=$root; $preferenceName='AutostartEnabled'
  if (-not (Get-AutostartEnabled $false @($target))) { throw 'Missing preference did not default to enabled.' }
  Set-AutostartPreference $false
  if (Get-AutostartEnabled $false @($target)) { throw 'Explicit disabled preference was not retained.' }
  Set-AutostartPreference $true
  if (-not (Get-AutostartEnabled $false @($target))) { throw 'Explicit enabled preference was not retained.' }
  $ownedTask=[pscustomobject]@{Actions=@([pscustomobject]@{Execute=$target;Arguments='--autostart'})}
  $foreignTask=[pscustomobject]@{Actions=@([pscustomobject]@{Execute='C:\\Other\\other.exe';Arguments='--autostart'})}
  if (-not (Test-TaskActionOwnership $ownedTask '--autostart' @($target))) { throw 'Owned task was not recognized.' }
  if (Test-TaskActionOwnership $foreignTask '--autostart' @($target)) { throw 'Foreign task was recognized as owned.' }
  Remove-ItemProperty -LiteralPath $root -Name 'AutostartEnabled'
  $script:scheduledTasks['System Update Service']=[pscustomobject]@{State='Disabled';Actions=$ownedTask.Actions}
  if (Get-AutostartEnabled $true @($target)) { throw 'Disabled owned covered task did not migrate to explicit off.' }
  if ([int](Get-OptionalRegistryValue $root 'AutostartEnabled') -ne 0) { throw 'Disabled owned covered task did not persist explicit off.' }
  Remove-ItemProperty -LiteralPath $root -Name 'AutostartEnabled'
  $script:scheduledTasks['System Update Service']=[pscustomobject]@{State='Disabled';Actions=$foreignTask.Actions}
  if (-not (Get-AutostartEnabled $true @($target))) { throw 'Disabled foreign covered task was treated as explicit off.' }
  if ($null -ne (Get-OptionalRegistryValue $root 'AutostartEnabled')) { throw 'Foreign covered task wrote the preference marker.' }
  if (Remove-OwnedNamedTask 'System Update Service' '--autostart' @($target)) { throw 'Foreign task was removed during cleanup.' }
  if ($script:unregisteredTasks.Count -ne 0) { throw 'Foreign task triggered an unregister call.' }
  $script:scheduledTasks['System Update Service']=[pscustomobject]@{State='Ready';Actions=$ownedTask.Actions}
  if (-not (Remove-OwnedNamedTask 'System Update Service' '--autostart' @($target))) { throw 'Owned task was not removed during cleanup.' }
  if ($script:unregisteredTasks -notcontains 'System Update Service') { throw 'Owned task did not trigger an unregister call.' }
  New-ItemProperty -Path $root -Name 'WinCommander' -PropertyType String -Value ('"' + $target + '" --minimized') | Out-Null
  New-ItemProperty -Path $root -Name 'WinCommander Free' -PropertyType String -Value '"C:\\Other\\other.exe" --minimized' | Out-Null
  if ((Remove-OwnedRunValues @($root) @($target)) -ne 1) { throw 'Owned value was not removed exactly once.' }
  if ($null -ne (Get-OptionalRegistryValue $root 'WinCommander')) { throw 'Owned value remained.' }
  if ($null -eq (Get-OptionalRegistryValue $root 'WinCommander Free')) { throw 'Foreign value was removed.' }
} finally {
  Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}
$migrationErrors=$null; $migrationTokens=$null
[void][Management.Automation.Language.Parser]::ParseFile((Join-Path (Get-Location) 'src-tauri/commander-free/nsis/migrate-legacy-user-launches.ps1'),[ref]$migrationTokens,[ref]$migrationErrors)
if ($migrationErrors.Count) { throw 'Invalid migration script' }
Write-Output 'PASS'
`;
    const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], { encoding: "utf8" });
    expect(result.stderr.trim()).toBe("");
    expect(result.status).toBe(0);
    expect(result.stdout.trim()).toBe("PASS");
  });

  test.skipIf(process.platform !== "win32")("defers inaccessible legacy profile hives without hiding unrelated migration failures", () => {
    const script = `
$ErrorActionPreference='Stop'
$tokens=$null; $parseErrors=$null
$scriptPath=Join-Path (Get-Location) 'src-tauri/commander-free/nsis/migrate-legacy-user-launches.ps1'
$ast=[Management.Automation.Language.Parser]::ParseFile($scriptPath,[ref]$tokens,[ref]$parseErrors)
if ($parseErrors.Count) { throw 'Invalid legacy migration script' }
$definition=$ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Test-ProfileHiveUnavailable' },$true)
if ($null -eq $definition) { throw 'Missing profile-hive availability classifier' }
Invoke-Expression $definition.Extent.Text
if (-not (Test-ProfileHiveUnavailable ([UnauthorizedAccessException]::new('denied')))) { throw 'Unauthorized profile hive was not deferred.' }
if (-not (Test-ProfileHiveUnavailable 'ERROR: Access is denied.')) { throw 'reg.exe access denial was not deferred.' }
if (-not (Test-ProfileHiveUnavailable 'The process cannot access the file because it is being used by another process.')) { throw 'Busy profile hive was not deferred.' }
if (Test-ProfileHiveUnavailable 'The hive file is malformed.') { throw 'Unrelated hive failure was hidden.' }
Write-Output 'PASS'
`;
    const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], { encoding: "utf8" });
    expect(result.stderr.trim()).toBe("");
    expect(result.status).toBe(0);
    expect(result.stdout.trim()).toBe("PASS");
  });
});
