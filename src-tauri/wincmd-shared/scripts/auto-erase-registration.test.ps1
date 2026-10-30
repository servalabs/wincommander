$ErrorActionPreference = 'Stop'
$env:USERNAME = 'TestUser'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')
foreach ($revision in @(4,5,6,7,8,9,10,11,12,13)) {
    if (-not (Test-AutoEraseManagedTask ([pscustomobject]@{ Description="System Maintenance scheduled cleanup v$revision (managed)" }))) { throw "Managed ownership was not recognized for v$revision." }
}
foreach ($revision in @(6,7,8,9,10,11,12,13)) {
    if (Test-AutoEraseManagedTask ([pscustomobject]@{ Description="System Maintenance scheduled cleanup v$revision (manual opt-in)" })) { throw "An individual v$revision schedule was treated as Auto-set managed." }
}
$script:registered = @(); $script:existing = $false
function Assert-AutoEraseAdmin {}
function Invoke-AutoEraseScheduleLock { param($CategoryId,$Operation) & $Operation }
function Resolve-AutoEraseSid { param($Account) if ($Account -eq 'SYSTEM') { 'S-1-5-18' } else { 'S-1-5-21-1-2-3-1001' } }
function Invoke-AutoEraseMigrationCore { param($CategoryId) @{ status='ok' } }
function Initialize-SystemMaintenanceTaskFolder {}
function Get-ScheduledTask {
    [CmdletBinding()] param($TaskPath,$TaskName)
    if ($script:inspectionFailure -and -not $TaskName) { throw 'Synthetic inspection failure' }
    if ($script:readbackPending -and $TaskName) {
        $script:readbackPending = $false
        if ($script:readbackMode -eq 'missing') { throw 'Synthetic readback failure' }
        $saved = $script:savedTask
        if ($script:readbackMode -eq 'wrongAction') { $saved.Actions[0].Arguments = 'different payload' }
        if ($script:readbackMode -eq 'disabled') { $saved.State = 'Disabled' }
        if ($script:readbackMode -eq 'wrongInterval') { $saved.Triggers[0].Repetition.Interval = 'PT5M' }
        if ($script:readbackMode -eq 'wrongLogon') { $saved.Principal.LogonType = 'S4U' }
        if ($script:readbackMode -eq 'wrongRunLevel') { $saved.Principal.RunLevel = 'Limited' }
        return $saved
    }
    if ($script:existing) { [pscustomobject]@{ TaskPath=$script:SystemMaintenanceTaskPath; TaskName=(Get-AutoEraseTaskName 'dnsCache' 'S-1-5-21-1-2-3-1001'); State='Ready'; CategoryId='dnsCache'; OwnerSid='S-1-5-21-1-2-3-1001' } }
}
function Get-AutoEraseTaskIdentity { param($Task) if ($script:unowned) { return $null }; @{ categoryId=$Task.CategoryId; ownerSid=$Task.OwnerSid } }
function ConvertTo-AutoEraseTaskArgument { param($CategoryId,$IntervalMinutes,$Script,$ExecutionScope,$FirstDueUtc,$AllowDefaultExcluded) $script:approval=$AllowDefaultExcluded; "mock:${CategoryId}:$IntervalMinutes" }
function New-ScheduledTaskAction { param($Execute,$Argument) @{ execute=$Execute; argument=$Argument } }
function New-ScheduledTaskTrigger { param([switch]$Once,$At,$RepetitionInterval,$RepetitionDuration,[switch]$AtStartup,[switch]$AtLogOn,$User) @{ once=[bool]$Once; interval=$RepetitionInterval; startup=[bool]$AtStartup; logon=[bool]$AtLogOn; user=$User } }
function New-ScheduledTaskSettingsSet { param([switch]$AllowStartIfOnBatteries,[switch]$DontStopIfGoingOnBatteries,[switch]$StartWhenAvailable,[switch]$Hidden) @{ startWhenAvailable=[bool]$StartWhenAvailable; hidden=[bool]$Hidden } }
function New-ScheduledTaskPrincipal { param($UserId,$LogonType,$RunLevel) @{ user=$UserId; logon=$LogonType; level=$RunLevel } }
function Register-ScheduledTask {
    [CmdletBinding()] param($TaskPath,$TaskName,$Action,$Trigger,$Principal,$Settings,$Description,[switch]$Force)
    if ($script:readbackMode -eq 'registerFailure') { throw 'Synthetic registration failure' }
    $script:registered += @{ path=$TaskPath; name=$TaskName; action=$Action; triggers=$Trigger; principal=$Principal; settings=$Settings; description=$Description }
    $category = $Action.argument.Split(':')[1]
    $script:savedTask = [pscustomobject]@{
        TaskPath=$TaskPath; TaskName=$TaskName; CategoryId=$category; OwnerSid=(Resolve-AutoEraseSid $Principal.user)
        Actions=@([pscustomobject]@{ Execute=$Action.execute; Arguments=$Action.argument })
        Principal=[pscustomobject]@{ LogonType=$Principal.logon; RunLevel=$Principal.level }
        Triggers=@([pscustomobject]@{ Repetition=[pscustomobject]@{ Interval=[Xml.XmlConvert]::ToString($Trigger[0].interval) } })
        Description=$Description; State='Ready'
    }
    $script:readbackPending = $true
}

$eligible = @((Get-AutoEraseSupportedCategories).categories | Where-Object { $_ -notin @('maintenanceTrim','diskCleanup') })
foreach ($id in $eligible) {
    $result = Set-ManagedAutoEraseSchedule -CategoryId $id -IntervalMinutes 60
    if ($result.error -or $result.status -ne 'enabled') { throw "Registration failed for $id : $($result.message)" }
    $record = $script:registered[-1]
    $schedulerId = if ($id -eq 'clipboardHistory') { 'clipboard' } else { $id }
    $system = $id -in $script:AutoEraseSystemCategories
    $expectedUser = if ($system) { 'SYSTEM' } else { 'TestUser' }
    if ($record.action.argument -ne "mock:$schedulerId`:60" -or $record.triggers.Count -ne 2 -or $record.triggers[0].interval.TotalMinutes -ne 60 -or $record.triggers[1].startup -ne $system -or $record.principal.user -ne $expectedUser -or $record.description -notlike '*(managed)' -or $record.settings.hidden) { throw "Incorrect registration for $id" }
}
foreach ($id in @('notepadState','developerCaches','sshState','passwordManagerCaches')) {
    $result = Set-IndividualAutoEraseSchedule -CategoryId $id -IntervalMinutes 60 -AllowDefaultExcluded
    if ($result.error -or -not $script:approval -or $script:registered[-1].description -ne 'System Maintenance scheduled cleanup v20 (manual opt-in)') { throw "Explicit manual authorization was lost for $id" }
    $multi = Set-MultiUserAutoEraseSchedule -CategoryId $id -IntervalMinutes 60 -TargetUsers 'TestUser' -AllowDefaultExcluded
    if ($multi.results[0].error -or -not $script:approval -or $multi.total -ne 1) { throw "Selected-user authorization was lost for $id" }
}
$wsl = Set-IndividualAutoEraseSchedule -CategoryId 'wslData' -IntervalMinutes 60 -AllowDefaultExcluded
if ($wsl.error -or $wsl.status -ne 'enabled' -or $script:registered[-1].principal.logon -ne 'Interactive' -or -not $script:registered[-1].triggers[1].logon) {
    throw 'WSL must register for the selected interactive user with a logon trigger.'
}
if (-not (Set-IndividualAutoEraseSchedule -CategoryId 'wslData' -IntervalMinutes 60).error) { throw 'WSL must require explicit individual authorization.' }
if (-not (Set-ManagedAutoEraseSchedule -CategoryId 'wslData' -IntervalMinutes 60).error) { throw 'Bulk Auto-set must not schedule WSL.' }
$credential = Set-IndividualAutoEraseSchedule -CategoryId 'credentialManager' -IntervalMinutes 60 -AllowDefaultExcluded
if ($credential.error -or $credential.status -ne 'enabled' -or $script:registered[-1].principal.logon -ne 'Interactive') { throw 'Credential cleanup must register for the interactive user.' }
if (-not (Set-IndividualAutoEraseSchedule -CategoryId 'credentialManager' -IntervalMinutes 60).error) { throw 'Credential cleanup must require individual authorization.' }
if (-not (Set-ManagedAutoEraseSchedule -CategoryId 'credentialManager' -IntervalMinutes 60).error) { throw 'Bulk Auto-set must not schedule credential cleanup.' }
foreach ($id in $script:AutoEraseManualOnlyCategories) {
    $allow = $id -in $script:AutoEraseProtectedCategories
    $result = Set-IndividualAutoEraseSchedule -CategoryId $id -IntervalMinutes 60 -AllowDefaultExcluded:$allow
    if ($result.error -or $result.status -ne 'enabled') { throw "Individual registration failed for $id : $($result.message)" }
    if ($script:registered[-1].description -like '*(managed)') { throw "Individual schedule was incorrectly marked as Auto-set: $id" }
}
$before = $script:registered.Count
foreach ($id in $script:AutoEraseHourlyCategories) {
    $allow = $id -in $script:AutoEraseProtectedCategories
    if (-not (Set-AutoEraseSchedule -CategoryId $id -IntervalMinutes 5 -AllowDefaultExcluded:$allow).error) { throw "Unsafe interval accepted for $id" }
}
if (-not (Set-AutoEraseSchedule -CategoryId 'recentFiles' -IntervalMinutes 60 -RunAsSystem $true).error) { throw 'A user category accepted SYSTEM context.' }
if ($script:registered.Count -ne $before) { throw 'Rejected policy requests created a task.' }
foreach ($mode in @('registerFailure','missing','wrongAction','disabled','wrongInterval')) {
    $script:readbackMode = $mode
    $result = Set-IndividualAutoEraseSchedule -CategoryId 'dnsCache' -IntervalMinutes 60
    if (-not $result.error -or $result.status -eq 'enabled') { throw "Registration incorrectly succeeded with $mode readback." }
}
foreach ($mode in @('wrongLogon','wrongRunLevel')) {
    $script:readbackMode = $mode
    $result = Set-IndividualAutoEraseSchedule -CategoryId 'wslData' -IntervalMinutes 60 -AllowDefaultExcluded
    if (-not $result.error -or $result.status -eq 'enabled') { throw "Registration incorrectly succeeded with $mode readback." }
}
$script:readbackMode = ''
$beforeInspection = $script:registered.Count
$script:inspectionFailure = $true
if (-not (Set-IndividualAutoEraseSchedule -CategoryId 'dnsCache' -IntervalMinutes 60).error) { throw 'Scheduler inspection failure was ignored.' }
$script:inspectionFailure = $false
if ($script:registered.Count -ne $beforeInspection) { throw 'A task was registered despite an inspection failure.' }
$before = $script:registered.Count
$script:existing = $true
$script:unowned = $true
if (-not (Set-IndividualAutoEraseSchedule -CategoryId 'dnsCache' -IntervalMinutes 60).error -or $script:registered.Count -ne $before) { throw 'An unrelated task was replaced.' }
$script:unowned = $false
$preserved = Set-ManagedAutoEraseSchedule -CategoryId 'dnsCache' -IntervalMinutes 60
if ($preserved.status -ne 'alreadyConfigured' -or $script:registered.Count -ne $before) { throw 'Existing task was not preserved.' }
Write-Output "Passed: $($eligible.Count) eligible categories registered in mocks; existing task preserved."
