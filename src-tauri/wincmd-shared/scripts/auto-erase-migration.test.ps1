$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')
function Assert-AutoEraseAdmin {}
function Initialize-SystemMaintenanceTaskFolder {}
function ConvertTo-AutoEraseComparableTaskXml { param([xml]$Document) $Document }
function Get-AutoEraseTaskIdentity { param($Task) @{ categoryId=$Task.Category; ownerSid='S-1-5-21-1-2-3-1001'; taskName=$Task.TaskName } }
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
    if ($PayloadRevision -ne 6 -or $IntervalMinutes -ne 37 -or $ExecutionScope -ne 'user-TestUser') { throw 'Migration changed schedule or user context.' }
    '-NoProfile -File "mock-revision6.ps1"'
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
if ($after.Task.RegistrationInfo.Description -ne 'System Maintenance scheduled cleanup v6 (managed)') { throw 'Managed ownership was lost.' }
$script:task.Category = 'browserFootprints'; $script:task.State = 'Ready'
$script:task.TaskName = 'ProtectedSyntheticTask'; $script:registrations = 0
$result = Invoke-AutoEraseMigration -CategoryId 'browserFootprints'
if ($result.error -or $script:disabled.Count -ne 1 -or $script:registrations -or $result.disabledProtected.Count -ne 1) { throw 'Protected legacy task was not disabled without rewriting its payload.' }
Write-Output 'Passed: v4 payload upgrade preserves interval, triggers, principal and disabled settings; protected legacy task disabled in mocks.'
