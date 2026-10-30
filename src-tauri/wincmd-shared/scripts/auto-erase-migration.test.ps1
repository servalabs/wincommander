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
    @{ NextRunTime=[DateTime]::Parse('2026-11-02T01:00:00Z') }
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
    if ($PayloadRevision -ne 18 -or $IntervalMinutes -ne 37 -or $ExecutionScope -ne 'user-TestUser') { throw 'Migration changed schedule or user context.' }
    if ($CategoryId -eq 'browserFootprints' -and -not $AllowDefaultExcluded) { throw 'Migration lost explicit per-card authorization.' }
    '-NoProfile -File "mock-revision11.ps1"'
}
$script:disabled = @(); $script:registrations = 0; $script:registeredXml = ''
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
$result = Invoke-AutoEraseMigration -CategoryId 'dnsCache'
if ($result.error -or $script:registrations -ne 1 -or $result.migrated.Count -ne 1) { throw "v4 upgrade failed: $($result.message)" }
[xml]$before = $script:originalXml; [xml]$after = $script:registeredXml
foreach ($section in @('Triggers','Principals','Settings')) {
    if ($before.Task.$section.OuterXml -ne $after.Task.$section.OuterXml) { throw "Migration changed $section" }
}
if ($after.Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (managed)') { throw 'Managed ownership was lost.' }
$script:task.Description = 'System Maintenance scheduled cleanup v5 (managed)'
$script:originalXml = $script:originalXml -replace 'v4 \(managed\)', 'v5 (managed)'
$script:registeredXml = ''; $script:registrations = 0
$upgradedV5 = Invoke-AutoEraseMigration -CategoryId 'dnsCache'
if ($upgradedV5.error -or $script:registrations -ne 1 -or $upgradedV5.migrated.Count -ne 1) { throw 'Existing v5 managed schedule must receive the updated cleanup payload.' }
[xml]$upgradedV5Xml = $script:registeredXml
if ($upgradedV5Xml.Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (managed)' -or $upgradedV5Xml.Task.Settings.Enabled -ne 'false' -or $upgradedV5Xml.Task.Triggers.OuterXml -ne $before.Task.Triggers.OuterXml -or $upgradedV5Xml.Task.Principals.OuterXml -ne $before.Task.Principals.OuterXml) { throw 'The v5 migration lost managed ownership, schedule, owner or disabled state.' }
$script:task.Description = 'System Maintenance scheduled cleanup v7 (managed)'
$script:originalXml = $script:registeredXml -replace 'v18 \(managed\)', 'v7 (managed)'
$script:registeredXml = ''; $script:registrations = 0
$upgradedV7 = Invoke-AutoEraseMigration -CategoryId 'dnsCache'
if ($upgradedV7.error -or $script:registrations -ne 1 -or $upgradedV7.migrated.Count -ne 1) { throw 'Existing v7 managed schedule must receive the updated cleanup payload.' }
[xml]$upgradedV7Xml = $script:registeredXml
if ($upgradedV7Xml.Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (managed)' -or $upgradedV7Xml.Task.Settings.Enabled -ne 'false' -or $upgradedV7Xml.Task.Triggers.OuterXml -ne $before.Task.Triggers.OuterXml -or $upgradedV7Xml.Task.Principals.OuterXml -ne $before.Task.Principals.OuterXml) { throw 'The v7 migration lost managed ownership, schedule, owner or disabled state.' }
$script:task.Description = 'System Maintenance scheduled cleanup v8 (managed)'
$script:originalXml = $script:registeredXml -replace 'v18 \(managed\)', 'v8 (managed)'
$script:registeredXml = ''; $script:registrations = 0
$upgradedV8 = Invoke-AutoEraseMigration -CategoryId 'dnsCache'
if ($upgradedV8.error -or $script:registrations -ne 1 -or $upgradedV8.migrated.Count -ne 1) { throw 'Existing v8 managed schedule must receive the updated cleanup payload.' }
[xml]$upgradedV8Xml = $script:registeredXml
if ($upgradedV8Xml.Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (managed)' -or $upgradedV8Xml.Task.Settings.Enabled -ne 'false' -or $upgradedV8Xml.Task.Triggers.OuterXml -ne $before.Task.Triggers.OuterXml -or $upgradedV8Xml.Task.Principals.OuterXml -ne $before.Task.Principals.OuterXml) { throw 'The v8 migration lost managed ownership, schedule, owner or disabled state.' }
$script:task.Description = 'System Maintenance scheduled cleanup v9 (managed)'
$script:originalXml = $script:registeredXml -replace 'v18 \(managed\)', 'v9 (managed)'
$script:registeredXml = ''; $script:registrations = 0
$upgradedV9 = Invoke-AutoEraseMigration -CategoryId 'dnsCache'
if ($upgradedV9.error -or $script:registrations -ne 1 -or $upgradedV9.migrated.Count -ne 1) { throw 'Existing v9 managed schedule must receive the updated cleanup payload.' }
[xml]$upgradedV9Xml = $script:registeredXml
if ($upgradedV9Xml.Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (managed)' -or $upgradedV9Xml.Task.Settings.Enabled -ne 'false' -or $upgradedV9Xml.Task.Triggers.OuterXml -ne $before.Task.Triggers.OuterXml -or $upgradedV9Xml.Task.Principals.OuterXml -ne $before.Task.Principals.OuterXml) { throw 'The v9 migration lost managed ownership, schedule, owner or disabled state.' }
$script:task.Description = 'System Maintenance scheduled cleanup v10 (managed)'
$script:originalXml = $script:registeredXml -replace 'v18 \(managed\)', 'v10 (managed)'
$script:registeredXml = ''; $script:registrations = 0
$upgradedV10 = Invoke-AutoEraseMigration -CategoryId 'dnsCache'
if ($upgradedV10.error -or $script:registrations -ne 1 -or $upgradedV10.migrated.Count -ne 1) { throw 'Existing v10 managed schedule must receive the updated cleanup payload.' }
[xml]$upgradedV10Xml = $script:registeredXml
if ($upgradedV10Xml.Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (managed)' -or $upgradedV10Xml.Task.Settings.Enabled -ne 'false' -or $upgradedV10Xml.Task.Triggers.OuterXml -ne $before.Task.Triggers.OuterXml -or $upgradedV10Xml.Task.Principals.OuterXml -ne $before.Task.Principals.OuterXml) { throw 'The v10 migration lost managed ownership, schedule, owner or disabled state.' }
$script:task.Description = 'System Maintenance scheduled cleanup v11 (managed)'
$script:originalXml = $script:registeredXml -replace 'v18 \(managed\)', 'v11 (managed)'
$script:registeredXml = ''; $script:registrations = 0
$upgradedV11 = Invoke-AutoEraseMigration -CategoryId 'dnsCache'
if ($upgradedV11.error -or $script:registrations -ne 1 -or $upgradedV11.migrated.Count -ne 1 -or ([xml]$script:registeredXml).Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (managed)') { throw 'Existing v11 managed schedule was not upgraded.' }
$script:task.Description = 'System Maintenance scheduled cleanup v12 (managed)'
$script:originalXml = $script:registeredXml -replace 'v18 \(managed\)', 'v12 (managed)'
$script:registeredXml = ''; $script:registrations = 0
$upgradedV12 = Invoke-AutoEraseMigration -CategoryId 'dnsCache'
[xml]$upgradedV12Xml = $script:registeredXml
if ($upgradedV12.error -or $script:registrations -ne 1 -or $upgradedV12.migrated.Count -ne 1 -or $upgradedV12Xml.Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (managed)' -or $upgradedV12Xml.Task.Settings.Enabled -ne 'false' -or $upgradedV12Xml.Task.Triggers.OuterXml -ne $before.Task.Triggers.OuterXml -or $upgradedV12Xml.Task.Principals.OuterXml -ne $before.Task.Principals.OuterXml) { throw 'The v12 managed migration lost ownership, schedule, owner or disabled state.' }
$script:task.Description = 'System Maintenance scheduled cleanup v13 (managed)'
$script:originalXml = $script:registeredXml -replace 'v18 \(managed\)', 'v13 (managed)'
$script:registeredXml = ''; $script:registrations = 0
$upgradedV13 = Invoke-AutoEraseMigration -CategoryId 'dnsCache'
[xml]$upgradedV13Xml = $script:registeredXml
if ($upgradedV13.error -or $script:registrations -ne 1 -or $upgradedV13.migrated.Count -ne 1 -or $upgradedV13Xml.Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (managed)' -or $upgradedV13Xml.Task.Settings.Enabled -ne 'false' -or $upgradedV13Xml.Task.Triggers.OuterXml -ne $before.Task.Triggers.OuterXml -or $upgradedV13Xml.Task.Principals.OuterXml -ne $before.Task.Principals.OuterXml) { throw 'The v13 migration lost authorization, schedule, owner or disabled state.' }
$script:originalXml = $script:registeredXml -replace 'v18 \(managed\)', 'v14 (managed)'
$script:registeredXml = ''; $script:registrations = 0
$script:task.Description = 'System Maintenance scheduled cleanup v14 (managed)'
$upgradedV14 = Invoke-AutoEraseMigration -CategoryId 'dnsCache'
if ($upgradedV14.error -or $script:registrations -ne 1 -or ([xml]$script:registeredXml).Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (managed)') { throw 'Existing v14 managed schedule was not upgraded.' }
$script:originalXml = $script:registeredXml -replace 'v18 \(managed\)', 'v15 (managed)'
$script:registrations = 0; $script:registeredXml = ''
$script:task.Description = 'System Maintenance scheduled cleanup v15 (managed)'
$upgradedV15 = Invoke-AutoEraseMigration -CategoryId 'dnsCache'
if ($upgradedV15.error -or $script:registrations -ne 1 -or ([xml]$script:registeredXml).Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (managed)') { throw 'Existing v15 schedule was not upgraded with its ownership retained.' }
$script:originalXml = $script:registeredXml -replace 'v18 \(managed\)', 'v16 (managed)'
$script:registrations = 0; $script:registeredXml = ''
$script:task.Description = 'System Maintenance scheduled cleanup v16 (managed)'
$upgradedV16 = Invoke-AutoEraseMigration -CategoryId 'dnsCache'
if ($upgradedV16.error -or $script:registrations -ne 1 -or ([xml]$script:registeredXml).Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (managed)') { throw 'Existing v16 managed schedule was not upgraded.' }
$script:task.Description = 'System Maintenance scheduled cleanup v17 (managed)'
$script:originalXml = $script:registeredXml -replace 'v18 \(managed\)', 'v17 (managed)'
$script:registeredXml = ''; $script:registrations = 0
$upgradeV17 = Invoke-AutoEraseMigration -CategoryId $script:task.Category
if ($upgradeV17.error -or $script:registrations -ne 1 -or ([xml]$script:registeredXml).Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (managed)') { throw 'The v17 managed schedule failed to upgrade.' }
$script:task.Description = 'System Maintenance scheduled cleanup v18 (managed)'
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
$script:originalXml = $script:registeredXml -replace 'v18 \(manual opt-in\)', 'v7 (manual opt-in)'
$script:registeredXml = ''; $script:registrations = 0
$upgradedV7Manual = Invoke-AutoEraseMigration -CategoryId 'browserFootprints'
if ($upgradedV7Manual.error -or $script:registrations -ne 1 -or $upgradedV7Manual.migrated.Count -ne 1) { throw 'Existing v7 manual schedule must receive the updated cleanup payload.' }
[xml]$upgradedV7ManualXml = $script:registeredXml
if ($upgradedV7ManualXml.Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (manual opt-in)' -or $upgradedV7ManualXml.Task.Settings.Enabled -ne 'false' -or $upgradedV7ManualXml.Task.Triggers.OuterXml -ne $before.Task.Triggers.OuterXml -or $upgradedV7ManualXml.Task.Principals.OuterXml -ne $before.Task.Principals.OuterXml) { throw 'The v7 migration lost consent, schedule, owner or disabled state.' }
$script:task.Description = 'System Maintenance scheduled cleanup v8 (manual opt-in)'
$script:originalXml = $script:registeredXml -replace 'v18 \(manual opt-in\)', 'v8 (manual opt-in)'
$script:registeredXml = ''; $script:registrations = 0
$upgradedV8Manual = Invoke-AutoEraseMigration -CategoryId 'browserFootprints'
if ($upgradedV8Manual.error -or $script:registrations -ne 1 -or $upgradedV8Manual.migrated.Count -ne 1) { throw 'Existing v8 manual schedule must receive the updated cleanup payload.' }
[xml]$upgradedV8ManualXml = $script:registeredXml
if ($upgradedV8ManualXml.Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (manual opt-in)' -or $upgradedV8ManualXml.Task.Settings.Enabled -ne 'false' -or $upgradedV8ManualXml.Task.Triggers.OuterXml -ne $before.Task.Triggers.OuterXml -or $upgradedV8ManualXml.Task.Principals.OuterXml -ne $before.Task.Principals.OuterXml) { throw 'The v8 migration lost consent, schedule, owner or disabled state.' }
$script:task.Description = 'System Maintenance scheduled cleanup v9 (manual opt-in)'
$script:originalXml = $script:registeredXml -replace 'v18 \(manual opt-in\)', 'v9 (manual opt-in)'
$script:registeredXml = ''; $script:registrations = 0
$upgradedV9Manual = Invoke-AutoEraseMigration -CategoryId 'browserFootprints'
if ($upgradedV9Manual.error -or $script:registrations -ne 1 -or $upgradedV9Manual.migrated.Count -ne 1) { throw 'Existing v9 manual schedule must receive the updated cleanup payload.' }
[xml]$upgradedV9ManualXml = $script:registeredXml
if ($upgradedV9ManualXml.Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (manual opt-in)' -or $upgradedV9ManualXml.Task.Settings.Enabled -ne 'false' -or $upgradedV9ManualXml.Task.Triggers.OuterXml -ne $before.Task.Triggers.OuterXml -or $upgradedV9ManualXml.Task.Principals.OuterXml -ne $before.Task.Principals.OuterXml) { throw 'The v9 migration lost consent, schedule, owner or disabled state.' }
$script:task.Description = 'System Maintenance scheduled cleanup v10 (manual opt-in)'
$script:originalXml = $script:registeredXml -replace 'v18 \(manual opt-in\)', 'v10 (manual opt-in)'
$script:registeredXml = ''; $script:registrations = 0
$upgradedV10Manual = Invoke-AutoEraseMigration -CategoryId 'browserFootprints'
if ($upgradedV10Manual.error -or $script:registrations -ne 1 -or $upgradedV10Manual.migrated.Count -ne 1) { throw 'Existing v10 manual schedule must receive the updated cleanup payload.' }
[xml]$upgradedV10ManualXml = $script:registeredXml
if ($upgradedV10ManualXml.Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (manual opt-in)' -or $upgradedV10ManualXml.Task.Settings.Enabled -ne 'false' -or $upgradedV10ManualXml.Task.Triggers.OuterXml -ne $before.Task.Triggers.OuterXml -or $upgradedV10ManualXml.Task.Principals.OuterXml -ne $before.Task.Principals.OuterXml) { throw 'The v10 migration lost consent, schedule, owner or disabled state.' }
$script:task.Description = 'System Maintenance scheduled cleanup v11 (manual opt-in)'
$script:originalXml = $script:registeredXml -replace 'v18 \(manual opt-in\)', 'v11 (manual opt-in)'
$script:registeredXml = ''; $script:registrations = 0
$upgradedV11Manual = Invoke-AutoEraseMigration -CategoryId 'browserFootprints'
if ($upgradedV11Manual.error -or $script:registrations -ne 1 -or $upgradedV11Manual.migrated.Count -ne 1 -or ([xml]$script:registeredXml).Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (manual opt-in)') { throw 'Existing v11 manual schedule lost consent or was not upgraded.' }
$script:task.Description = 'System Maintenance scheduled cleanup v12 (manual opt-in)'
$script:originalXml = $script:registeredXml -replace 'v18 \(manual opt-in\)', 'v12 (manual opt-in)'
$script:registeredXml = ''; $script:registrations = 0
$upgradedV12Manual = Invoke-AutoEraseMigration -CategoryId 'browserFootprints'
[xml]$upgradedV12ManualXml = $script:registeredXml
if ($upgradedV12Manual.error -or $script:registrations -ne 1 -or $upgradedV12Manual.migrated.Count -ne 1 -or $upgradedV12ManualXml.Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (manual opt-in)' -or $upgradedV12ManualXml.Task.Settings.Enabled -ne 'false' -or $upgradedV12ManualXml.Task.Triggers.OuterXml -ne $before.Task.Triggers.OuterXml -or $upgradedV12ManualXml.Task.Principals.OuterXml -ne $before.Task.Principals.OuterXml) { throw 'The v12 manual migration lost consent, schedule, owner or disabled state.' }
$script:task.Description = 'System Maintenance scheduled cleanup v13 (manual opt-in)'
$script:originalXml = $script:registeredXml -replace 'v18 \(manual opt-in\)', 'v13 (manual opt-in)'
$script:registeredXml = ''; $script:registrations = 0
$upgradedV13 = Invoke-AutoEraseMigration -CategoryId 'browserFootprints'
[xml]$upgradedV13Xml = $script:registeredXml
if ($upgradedV13.error -or $script:registrations -ne 1 -or $upgradedV13.migrated.Count -ne 1 -or $upgradedV13Xml.Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (manual opt-in)' -or $upgradedV13Xml.Task.Settings.Enabled -ne 'false' -or $upgradedV13Xml.Task.Triggers.OuterXml -ne $before.Task.Triggers.OuterXml -or $upgradedV13Xml.Task.Principals.OuterXml -ne $before.Task.Principals.OuterXml) { throw 'The v13 migration lost authorization, schedule, owner or disabled state.' }
$script:originalXml = $script:registeredXml -replace 'v18 \(manual opt-in\)', 'v14 (manual opt-in)'
$script:registeredXml = ''; $script:registrations = 0
$script:task.Description = 'System Maintenance scheduled cleanup v14 (manual opt-in)'
$upgradedV14Manual = Invoke-AutoEraseMigration -CategoryId 'browserFootprints'
if ($upgradedV14Manual.error -or $script:registrations -ne 1 -or ([xml]$script:registeredXml).Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (manual opt-in)') { throw 'Existing v14 manual schedule lost consent or was not upgraded.' }
$script:originalXml = $script:registeredXml -replace 'v18 \(manual opt-in\)', 'v15 (manual opt-in)'
$script:registrations = 0; $script:registeredXml = ''
$script:task.Description = 'System Maintenance scheduled cleanup v15 (manual opt-in)'
$upgradedV15 = Invoke-AutoEraseMigration -CategoryId 'browserFootprints'
if ($upgradedV15.error -or $script:registrations -ne 1 -or ([xml]$script:registeredXml).Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (manual opt-in)') { throw 'Existing v15 schedule was not upgraded with its ownership retained.' }
$script:originalXml = $script:registeredXml -replace 'v18 \(manual opt-in\)', 'v16 (manual opt-in)'
$script:registrations = 0; $script:registeredXml = ''
$script:task.Description = 'System Maintenance scheduled cleanup v16 (manual opt-in)'
$upgradedV16 = Invoke-AutoEraseMigration -CategoryId 'browserFootprints'
if ($upgradedV16.error -or $script:registrations -ne 1 -or ([xml]$script:registeredXml).Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (manual opt-in)') { throw 'Existing v16 manual schedule lost consent or was not upgraded.' }
$script:task.Description = 'System Maintenance scheduled cleanup v17 (manual opt-in)'
$script:originalXml = $script:registeredXml -replace 'v18 \(manual opt-in\)', 'v17 (manual opt-in)'
$script:registeredXml = ''; $script:registrations = 0
$upgradeV17 = Invoke-AutoEraseMigration -CategoryId $script:task.Category
if ($upgradeV17.error -or $script:registrations -ne 1 -or ([xml]$script:registeredXml).Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v18 (manual opt-in)') { throw 'The v17 manual opt-in schedule failed to upgrade.' }
$script:task.Description = 'System Maintenance scheduled cleanup v18 (manual opt-in)'
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
if ($script:locks.Count -ne 30 -or @($script:locks | Where-Object { $_ -eq 'dnsCache' }).Count -ne 14 -or @($script:locks | Where-Object { $_ -eq 'browserFootprints' }).Count -ne 16) { throw 'Migration did not acquire the category lock.' }
Write-Output 'Passed: legacy upgrades, missing payload repair, access denial and protected schedules.'
