import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";

test.skipIf(process.platform !== "win32")("startup mutations reject foreign or ambiguous entries and enable the exact folder/WOW entry", () => {
  const script = String.raw`
$ErrorActionPreference = 'Stop'
. ./src-tauri/commander-free/scripts/modules/tweaks/startup-manager.ps1
function Assert-IsAdmin {}
$script:writes = @()
function Test-Path { param($Path,$LiteralPath) return $false }
function New-Item { param($Path,[switch]$Force,$ErrorAction) }
function Get-ItemProperty { param($Path,$LiteralPath,$Name,$ErrorAction) return $null }
function Set-ItemProperty { param($Path,$LiteralPath,$Name,$Value,$Type,[switch]$Force,$ErrorAction) $script:writes += [pscustomobject]@{ Path = if($LiteralPath){$LiteralPath}else{$Path}; Name=$Name; Value=$Value } }
function Get-StartupItems { return $script:entries }
function Entry($Name,$Location) { [pscustomobject]@{ Name=$Name; Location=$Location; Command='C:\Apps\app.exe'; IsEnabled=$false } }
$userRun='HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$machineRun='HKLM:\Software\Microsoft\Windows\CurrentVersion\Run'
$wow='HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run'
$script:entries=@((Entry 'Same' $userRun),(Entry 'Same' $machineRun))
$result=Enable-StartupItem -Name 'Same'
if ($result.success -or $script:writes.Count) { throw 'Ambiguous name changed a startup entry' }
$result=Enable-StartupItem -Name 'Same' -Location $machineRun
if (-not $result.success -or $script:writes[-1].Path -notlike 'HKLM:*') { throw 'Exact location was not used' }
$script:entries=@(Entry 'WowApp' $wow)
$result=Enable-StartupItem -Name 'WowApp' -Location $wow
if (-not $result.success -or $script:writes[-1].Path -notlike '*\Run32') { throw 'WOW entry did not enable through Run32' }
$shortcut=Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Startup\Example.lnk'
$script:entries=@(Entry 'Example' $shortcut)
$result=Enable-StartupItem -Name 'Example' -Location $shortcut
if (-not $result.success -or $script:writes[-1].Name -ne 'Example.lnk' -or $script:writes[-1].Path -notlike '*\StartupFolder') { throw 'Folder entry did not use its exact shortcut filename' }
$before=$script:writes.Count
$script:entries=@(Entry 'Foreign' 'C:\Temp\Startup\Foreign.lnk')
$result=Disable-StartupItem -Name 'Foreign' -Location 'C:\Temp\Startup\Foreign.lnk'
if ($result.success -or $script:writes.Count -ne $before) { throw 'Foreign location was accepted' }
$script:entries=@(Entry 'Foreign' 'HKCU:\Software\Foreign')
$result=Disable-StartupItem -Name 'Foreign' -Location 'HKCU:\Software\Foreign'
if ($result.success -or $script:writes.Count -ne $before) { throw 'Foreign registry key was accepted' }
$result=Enable-StartupItem -Name '*'
if ($result.success -or $script:writes.Count -ne $before) { throw 'Wildcard startup name was accepted' }
$script:entries=@(Entry 'Example' $shortcut)
function Test-Path { param($Path,$LiteralPath) return $LiteralPath -eq $shortcut }
function Get-Item { param($LiteralPath,[switch]$Force,$ErrorAction) return [pscustomobject]@{ Attributes=[IO.FileAttributes]::ReparsePoint } }
$result=Disable-StartupItem -Name 'Example' -Location $shortcut
if ($result.success -or $script:writes.Count -ne $before) { throw 'Linked shortcut was accepted' }
function Test-Path { param($Path,$LiteralPath) return $false }
$script:entries=@(Entry 'Same' $machineRun)
$script:removed=0
function Set-ItemProperty { param($Path,$LiteralPath,$Name,$Value,$Type,[switch]$Force,$ErrorAction) throw 'Access denied fixture' }
function Remove-ItemSecure { $script:removed++ }
$result=Disable-StartupItem -Name 'Same' -Location $machineRun
if ($result.success -or $script:removed) { throw 'Failed soft disable fell through to removal' }
$backupPath="$env:APPDATA\WinCommander\StartupBackups\disabled_items.json"
$script:entries=@((Entry 'Same' $userRun),(Entry 'Same' $machineRun))
$script:backups=@([pscustomobject]@{Name='Same';OriginalKey=$userRun;Type='Registry';Command='user.exe'},[pscustomobject]@{Name='Same';OriginalKey=$machineRun;Type='Registry';Command='machine.exe'})
function Test-Path { param($Path,$LiteralPath) return $LiteralPath -eq $backupPath }
function Get-Content { param($LiteralPath,[switch]$Raw) return ConvertTo-Json -InputObject $script:backups }
function Set-Content { param($LiteralPath,[Parameter(ValueFromPipeline=$true)]$Value) process { $script:remaining=$Value | ConvertFrom-Json } }
function New-ItemProperty { param($LiteralPath,$Name,$Value,$PropertyType,$ErrorAction) $script:restored=[pscustomobject]@{Path=$LiteralPath;Command=$Value} }
$result=Enable-StartupItem -Name 'Same' -Location $machineRun
if (-not $result.success -or $script:restored.Path -ne $machineRun -or $script:restored.Command -ne 'machine.exe') { throw "Restore selected another location with the same name: $($result.message) $($script:restored | ConvertTo-Json -Compress)" }
if (@($script:remaining).Count -ne 1 -or $script:remaining[0].OriginalKey -ne $userRun) { throw 'Restore removed a different entry backup' }
function Get-ItemProperty { param($Path,$LiteralPath,$Name,$ErrorAction) return [pscustomobject]@{Same='new-launcher.exe'} }
$script:restored=$null
$result=Enable-StartupItem -Name 'Same' -Location $machineRun
if ($result.success -or $script:restored) { throw 'Restore overwrote a newly registered launcher' }
Write-Output 'PASS'
`;
  const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], { encoding: "utf8" });
  expect(result.stderr).toBe("");
  expect(result.status).toBe(0);
  expect(result.stdout).toContain("PASS");
});
