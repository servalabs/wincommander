$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')
function Assert-AutoEraseAdmin {}
function Invoke-AutoEraseScheduleLock { param($CategoryId,$Operation) $script:locks += $CategoryId; & $Operation }
$script:locks = @()
function Initialize-SystemMaintenanceTaskFolder {}
function ConvertTo-AutoEraseComparableTaskXml { param([xml]$Document) $Document }
function Get-AutoEraseTaskIdentity { param($Task) @{ categoryId=$Task.Category; ownerSid='S-1-5-21-1-2-3-1001'; taskName=$Task.TaskName } }
$script:payloadState = 'valid'
function Test-AutoEraseCurrentPayload {
    param($Task, $Identity)
    if ($script:payloadState -eq 'denied') { throw 'Synthetic payload access denied.' }
    return $script:payloadState -eq 'valid'
}
function Get-ScheduledTask {
    [CmdletBinding()] param($TaskPath,$TaskName)
    $script:task
}
function Disable-ScheduledTask {
    [CmdletBinding()] param($TaskPath,$TaskName)
    $script:disabled += "$TaskPath$TaskName"; $script:task.State = 'Disabled'
}
function Get-ScheduledTaskInfo {
    [CmdletBinding()] param($TaskPath,$TaskName)
    @{ NextRunTime=$script:nextRunTime }
}
function Export-ScheduledTask {
    [CmdletBinding()] param($TaskPath,$TaskName)
    if ($script:registeredXml) { $script:registeredXml } else { $script:originalXml }
}
function Register-ScheduledTask {
    [CmdletBinding()] param($TaskPath,$TaskName,$Xml,[switch]$Force)
    $script:registeredXml = $Xml; $script:registrations++
}
function Enable-ScheduledTask { throw 'An originally disabled task must stay disabled.' }
function Unregister-ScheduledTask { throw 'A same-name payload migration must not remove the task.' }
function ConvertTo-AutoEraseTaskArgument {
    param($CategoryId,$IntervalMinutes,$Script,$ExecutionScope,$FirstDueUtc,$PayloadRevision,$AllowDefaultExcluded)
    if ($PayloadRevision -ne 22 -or $IntervalMinutes -ne 37 -or $ExecutionScope -ne 'user-TestUser') { throw 'Migration changed schedule or user context.' }
    if ($CategoryId -eq 'browserFootprints' -and -not $AllowDefaultExcluded) { throw 'Migration lost explicit per-card authorization.' }
    '-NoProfile -File "mock-revision11.ps1"'
}
$script:disabled = @(); $script:registrations = 0; $script:registeredXml = ''
$script:nextRunTime = [DateTime]::Parse('2026-11-02T01:00:00Z')
$script:task = [pscustomobject]@{
    Category='dnsCache'; TaskPath='\System Maintenance\'; TaskName='SM-U-08-12345678'; State='Disabled'
    Description='System Maintenance scheduled cleanup v4 (managed)'
    Principal=[pscustomobject]@{ UserId='TestUser' }
    Actions=@([pscustomobject]@{ Arguments='-File "C:\ProgramData\WinCommander\auto-erase\scripts\dnsCache.user-TestUser.ps1"' })
    Triggers=@([pscustomobject]@{ Repetition=[pscustomobject]@{ Interval='PT37M' } })
}
$script:originalXml = @'
<Task xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
<RegistrationInfo><Description>System Maintenance scheduled cleanup v4 (managed)</Description><URI>\SM-U-08-12345678</URI></RegistrationInfo>
<Triggers><TimeTrigger><StartBoundary>2026-10-01T13:21:00</StartBoundary><Repetition><Interval>PT37M</Interval></Repetition></TimeTrigger></Triggers>
<Principals><Principal><UserId>TestUser</UserId><LogonType>S4U</LogonType></Principal></Principals>
<Settings><Enabled>false</Enabled><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries></Settings>
<Actions><Exec><Command>powershell.exe</Command><Arguments>old-payload</Arguments></Exec></Actions>
</Task>
'@
foreach ($sentinel in @([DateTime]::MinValue, [datetime]'1899-12-30', [datetime]'1999-11-30')) {
    $script:nextRunTime = $sentinel
    $unknownNext = Invoke-AutoEraseMigration -CategoryId 'dnsCache'
    if (-not $unknownNext.error -or $unknownNext.message -notmatch 'dnsCache schedule next run could not be verified' -or $script:registrations -ne 0 -or $script:registeredXml) { throw 'An unavailable next run must not change the original task or guess its timing.' }
}
$script:locks = @()
$script:nextRunTime = [DateTime]::Parse('2026-11-02T01:00:00Z')
$result = Invoke-AutoEraseMigration -CategoryId 'dnsCache'
if ($result.error -or $script:registrations -ne 1 -or $result.migrated.Count -ne 1) { throw "v4 upgrade failed: $($result.message)" }
[xml]$before = $script:originalXml; [xml]$after = $script:registeredXml
foreach ($section in @('Triggers','Principals','Settings')) {
    if ($before.Task.$section.OuterXml -ne $after.Task.$section.OuterXml) { throw "Migration changed $section" }
}
if ($after.Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v22 (managed)') { throw 'Managed ownership was lost.' }
$script:task.Description = 'System Maintenance scheduled cleanup v5 (managed)'
$script:originalXml = $script:originalXml -replace 'v4 \(managed\)', 'v5 (managed)'
$script:registeredXml = ''; $script:registrations = 0
$upgradedV5 = Invoke-AutoEraseMigration -CategoryId 'dnsCache'
if ($upgradedV5.error -or $script:registrations -ne 1 -or $upgradedV5.migrated.Count -ne 1) { throw 'Existing v5 managed schedule must receive the updated cleanup payload.' }
[xml]$upgradedV5Xml = $script:registeredXml
if ($upgradedV5Xml.Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v22 (managed)' -or $upgradedV5Xml.Task.Settings.Enabled -ne 'false' -or $upgradedV5Xml.Task.Triggers.OuterXml -ne $before.Task.Triggers.OuterXml -or $upgradedV5Xml.Task.Principals.OuterXml -ne $before.Task.Principals.OuterXml) { throw 'The v5 migration lost managed ownership, schedule, owner or disabled state.' }
$script:task.Description = 'System Maintenance scheduled cleanup v7 (managed)'
$script:originalXml = $script:registeredXml -replace 'v22 \(managed\)', 'v7 (managed)'
$script:registeredXml = ''; $script:registrations = 0
$upgradedV7 = Invoke-AutoEraseMigration -CategoryId 'dnsCache'
if ($upgradedV7.error -or $script:registrations -ne 1 -or $upgradedV7.migrated.Count -ne 1) { throw 'Existing v7 managed schedule must receive the updated cleanup payload.' }
[xml]$upgradedV7Xml = $script:registeredXml
if ($upgradedV7Xml.Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v22 (managed)' -or $upgradedV7Xml.Task.Settings.Enabled -ne 'false' -or $upgradedV7Xml.Task.Triggers.OuterXml -ne $before.Task.Triggers.OuterXml -or $upgradedV7Xml.Task.Principals.OuterXml -ne $before.Task.Principals.OuterXml) { throw 'The v7 migration lost managed ownership, schedule, owner or disabled state.' }
foreach ($version in 8..21) {
    $script:task.Description = "System Maintenance scheduled cleanup v$version (managed)"
    $script:originalXml = $script:registeredXml -replace 'v22 \(managed\)', "v$version (managed)"
    $script:registeredXml = ''; $script:registrations = 0
    $upgraded = Invoke-AutoEraseMigration -CategoryId 'dnsCache'
    if ($upgraded.error -or $script:registrations -ne 1 -or $upgraded.migrated.Count -ne 1) { throw "Existing v$version managed schedule must receive the updated cleanup payload." }
    [xml]$upgradedXml = $script:registeredXml
    if ($upgradedXml.Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v22 (managed)' -or $upgradedXml.Task.Settings.Enabled -ne 'false' -or $upgradedXml.Task.Triggers.OuterXml -ne $before.Task.Triggers.OuterXml) { throw "The v$version managed schedule did not keep its ownership, enabled state and trigger." }
}
$script:task.Description = 'System Maintenance scheduled cleanup v22 (managed)'
$script:task | Add-Member -NotePropertyName Settings -NotePropertyValue ([pscustomobject]@{ Hidden=$true }) -Force
$script:originalXml = $script:registeredXml -replace '</Settings>', '<Hidden>true</Hidden></Settings>'
$script:registeredXml = ''; $script:registrations = 0
$visible = Invoke-AutoEraseMigration -CategoryId 'dnsCache'
if ($visible.error -or $script:registrations -ne 1) { throw 'An owned hidden task could not be made visible.' }
[xml]$visibleXml = $script:registeredXml
if ($visibleXml.Task.Settings.Hidden -ne 'false' -or $visibleXml.Task.Settings.Enabled -ne 'false' -or $visibleXml.Task.Triggers.OuterXml -ne $before.Task.Triggers.OuterXml -or $visibleXml.Task.Principals.OuterXml -ne $before.Task.Principals.OuterXml) { throw 'Visibility migration changed schedule, identity or enabled state.' }
$script:task.Category = 'browserFootprints'; $script:task.State = 'Disabled'
$script:task.TaskName = 'SM-U-09-12345678'; $script:task.Description = 'System Maintenance scheduled cleanup v6 (manual opt-in)'
$script:task.Settings.Hidden = $false
$script:task.Actions[0].Arguments = '-File "C:\ProgramData\WinCommander\auto-erase\scripts\browserFootprints.user-TestUser.ps1"'
$script:originalXml = $script:originalXml -replace 'v4 \(managed\)', 'v6 (manual opt-in)' -replace '<Hidden>true</Hidden>', '<Hidden>false</Hidden>'
$script:registeredXml = ''; $script:registrations = 0
$upgraded = Invoke-AutoEraseMigration -CategoryId 'browserFootprints'
if ($upgraded.error -or $script:registrations -ne 1 -or $upgraded.migrated.Count -ne 1) { throw 'Existing v6 manual schedule must receive the updated cleanup payload.' }
[xml]$upgradedXml = $script:registeredXml
if ($upgradedXml.Task.RegistrationInfo.Description -notmatch '\(manual opt-in\)$' -or $upgradedXml.Task.Settings.Enabled -ne 'false' -or $upgradedXml.Task.Triggers.OuterXml -ne $before.Task.Triggers.OuterXml -or $upgradedXml.Task.Principals.OuterXml -ne $before.Task.Principals.OuterXml) { throw 'The v6 migration lost consent, schedule, owner or disabled state.' }
$script:task.Description = 'System Maintenance scheduled cleanup v7 (manual opt-in)'
$script:originalXml = $script:registeredXml -replace 'v22 \(manual opt-in\)', 'v7 (manual opt-in)'
$script:registeredXml = ''; $script:registrations = 0
$upgradedV7Manual = Invoke-AutoEraseMigration -CategoryId 'browserFootprints'
if ($upgradedV7Manual.error -or $script:registrations -ne 1 -or $upgradedV7Manual.migrated.Count -ne 1) { throw 'Existing v7 manual schedule must receive the updated cleanup payload.' }
[xml]$upgradedV7ManualXml = $script:registeredXml
if ($upgradedV7ManualXml.Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v22 (manual opt-in)' -or $upgradedV7ManualXml.Task.Settings.Enabled -ne 'false' -or $upgradedV7ManualXml.Task.Triggers.OuterXml -ne $before.Task.Triggers.OuterXml -or $upgradedV7ManualXml.Task.Principals.OuterXml -ne $before.Task.Principals.OuterXml) { throw 'The v7 migration lost consent, schedule, owner or disabled state.' }
foreach ($version in 8..21) {
    $script:task.Description = "System Maintenance scheduled cleanup v$version (manual opt-in)"
    $script:originalXml = $script:registeredXml -replace 'v22 \(manual opt-in\)', "v$version (manual opt-in)"
    $script:registeredXml = ''; $script:registrations = 0
    $upgraded = Invoke-AutoEraseMigration -CategoryId 'browserFootprints'
    if ($upgraded.error -or $script:registrations -ne 1 -or $upgraded.migrated.Count -ne 1) { throw "Existing v$version manual schedule must receive the updated cleanup payload." }
    [xml]$upgradedXml = $script:registeredXml
    if ($upgradedXml.Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v22 (manual opt-in)' -or $upgradedXml.Task.Settings.Enabled -ne 'false' -or $upgradedXml.Task.Triggers.OuterXml -ne $before.Task.Triggers.OuterXml) { throw "The v$version manual schedule did not keep its ownership, enabled state and trigger." }
}
$script:task.Description = 'System Maintenance scheduled cleanup v22 (manual opt-in)'
$script:registrations = 0
$unchanged = Invoke-AutoEraseMigration -CategoryId 'browserFootprints'
if ($unchanged.error -or $script:registrations -or $unchanged.migrated.Count) { throw 'A current, visible schedule must not be rewritten.' }
$script:payloadState = 'missing'; $script:registrations = 0; $script:originalXml = $script:registeredXml; $script:registeredXml = ''
$repaired = Invoke-AutoEraseMigration -CategoryId 'browserFootprints'
if ($repaired.error -or $script:registrations -ne 1 -or $repaired.migrated.Count -ne 1) { throw 'A missing current payload was not repaired.' }
$script:payloadState = 'denied'; $script:registrations = 0
$denied = Invoke-AutoEraseMigration -CategoryId 'browserFootprints'
if (-not $denied.error -or $script:registrations -ne 0) { throw 'An unreadable payload must fail without replacing the task.' }
$script:payloadState = 'valid'
$script:task.Category = 'browserFootprints'; $script:task.State = 'Ready'
$script:task.Description = 'System Maintenance scheduled cleanup v5 (managed)'
$script:task.TaskName = 'ProtectedSyntheticTask'; $script:registrations = 0
$result = Invoke-AutoEraseMigration -CategoryId 'browserFootprints'
if ($result.error -or $script:disabled.Count -ne 1 -or $script:registrations -or $result.disabledProtected.Count -ne 1) { throw 'Protected legacy task was not disabled without rewriting its payload.' }
# The UI alias must select the canonical persisted clipboard task for upgrade.
$script:task.Category = 'clipboard'; $script:task.State = 'Disabled'; $script:task.TaskName = 'SM-U-08-12345678'
$script:task.Description = 'System Maintenance scheduled cleanup v4 (managed)'
$script:originalXml = $script:registeredXml -replace 'v22 \(manual opt-in\)', 'v4 (managed)'
$script:registeredXml = ''; $script:registrations = 0
$clipboardUpgrade = Invoke-AutoEraseMigration -CategoryId 'clipboardHistory'
if ($clipboardUpgrade.error -or $script:registrations -ne 1 -or @($clipboardUpgrade.migrated) -notcontains 'clipboard') { throw 'Clipboard alias did not select the canonical task for migration.' }
Write-Output 'Passed: legacy upgrades, missing payload repair, access denial and protected schedules.'
