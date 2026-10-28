$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')
$script:registered = @(); $script:existing = $false
function Assert-AutoEraseAdmin {}
function Resolve-AutoEraseSid { param($Account) 'S-1-5-21-1-2-3-1001' }
function Invoke-AutoEraseMigration { param($CategoryId) @{ status='ok' } }
function Initialize-SystemMaintenanceTaskFolder {}
function Get-ScheduledTask { [CmdletBinding()] param($TaskPath,$TaskName) if ($script:existing) { [pscustomobject]@{ Name=$TaskName } } }
function Get-AutoEraseTaskIdentity { param($Task) @{ categoryId='dnsCache' } }
function ConvertTo-AutoEraseTaskArgument { param($CategoryId,$IntervalMinutes,$Script,$ExecutionScope,$FirstDueUtc,$AllowDefaultExcluded) $script:approval=$AllowDefaultExcluded; "mock:${CategoryId}:$IntervalMinutes" }
function New-ScheduledTaskAction { param($Execute,$Argument) @{ execute=$Execute; argument=$Argument } }
function New-ScheduledTaskTrigger { param([switch]$Once,$At,$RepetitionInterval,$RepetitionDuration,[switch]$AtStartup,[switch]$AtLogOn,$User) @{ once=[bool]$Once; interval=$RepetitionInterval; startup=[bool]$AtStartup; logon=[bool]$AtLogOn; user=$User } }
function New-ScheduledTaskSettingsSet { param([switch]$AllowStartIfOnBatteries,[switch]$DontStopIfGoingOnBatteries,[switch]$StartWhenAvailable,[switch]$Hidden) @{ startWhenAvailable=[bool]$StartWhenAvailable } }
function New-ScheduledTaskPrincipal { param($UserId,$LogonType,$RunLevel) @{ user=$UserId; logon=$LogonType; level=$RunLevel } }
function Register-ScheduledTask {
    [CmdletBinding()] param($TaskPath,$TaskName,$Action,$Trigger,$Principal,$Settings,$Description,[switch]$Force)
    $script:registered += @{ path=$TaskPath; name=$TaskName; action=$Action; triggers=$Trigger; principal=$Principal; settings=$Settings; description=$Description }
}

$eligible = @((Get-AutoEraseSupportedCategories).categories | Where-Object { $_ -notin @('maintenanceTrim','diskCleanup') })
foreach ($id in $eligible) {
    $result = Set-AutoEraseSchedule -CategoryId $id -IntervalMinutes 60 -TargetUser 'TestUser' -PreserveExisting -ManagedByAutoSet
    if ($result.error -or $result.status -ne 'enabled') { throw "Registration failed for $id : $($result.message)" }
    $record = $script:registered[-1]
    $schedulerId = if ($id -eq 'clipboardHistory') { 'clipboard' } else { $id }
    $system = $id -in $script:AutoEraseSystemCategories
    $expectedUser = if ($system) { 'SYSTEM' } else { 'TestUser' }
    if ($record.action.argument -ne "mock:$schedulerId`:60" -or $record.triggers.Count -ne 2 -or $record.triggers[0].interval.TotalMinutes -ne 60 -or $record.triggers[1].startup -ne $system -or $record.principal.user -ne $expectedUser -or $record.description -notlike '*(managed)') { throw "Incorrect registration for $id" }
}
foreach ($id in @('notepadState','developerCaches','sshState','passwordManagerCaches')) {
    $result = Set-AutoEraseSchedule -CategoryId $id -IntervalMinutes 60 -TargetUser 'TestUser' -AllowDefaultExcluded
    if ($result.error -or -not $script:approval -or $script:registered[-1].description -ne 'System Maintenance scheduled cleanup v6 (manual opt-in)') { throw "Explicit manual authorization was lost for $id" }
    $multi = Set-MultiUserAutoEraseSchedule -CategoryId $id -IntervalMinutes 60 -TargetUsers 'TestUser' -AllowDefaultExcluded
    if ($multi.results[0].error -or -not $script:approval -or $multi.total -ne 1) { throw "Selected-user authorization was lost for $id" }
}
$before = $script:registered.Count
foreach ($id in $script:AutoEraseHourlyCategories) {
    $allow = $id -in $script:AutoEraseProtectedCategories
    if (-not (Set-AutoEraseSchedule -CategoryId $id -IntervalMinutes 5 -AllowDefaultExcluded:$allow).error) { throw "Unsafe interval accepted for $id" }
}
if (-not (Set-AutoEraseSchedule -CategoryId 'recentFiles' -IntervalMinutes 60 -RunAsSystem $true).error) { throw 'A user category accepted SYSTEM context.' }
if ($script:registered.Count -ne $before) { throw 'Rejected policy requests created a task.' }
$before = $script:registered.Count
$script:existing = $true
$preserved = Set-AutoEraseSchedule -CategoryId 'dnsCache' -IntervalMinutes 60 -TargetUser 'TestUser' -PreserveExisting -ManagedByAutoSet
if ($preserved.status -ne 'alreadyConfigured' -or $script:registered.Count -ne $before) { throw 'Existing task was not preserved.' }
Write-Output "Passed: $($eligible.Count) eligible categories registered in mocks; existing task preserved."
