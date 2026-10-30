$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')

# Synthetic scheduler boundary: these functions never access Windows tasks.
$env:USERNAME = 'SyntheticUser'
function Assert-AutoEraseAdmin {}
function Invoke-AutoEraseScheduleLock { param($CategoryId,$Operation) & $Operation }
function Resolve-AutoEraseSid { param($Account) 'S-1-5-21-1' }
$script:tasks = @()
$script:writes = 0
function Get-ScheduledTask { [CmdletBinding()] param() $script:tasks }
function Get-AutoEraseTaskIdentity { param($Task) if ($Task.Altered) { return $null }; @{ categoryId=$Task.Category; ownerSid=$Task.Owner } }
function Get-AutoEraseReadbackIdentity { param($Task) if ($Task.Altered) { return @{ categoryId=$Task.Category; ownerSid=$Task.Owner } }; return $null }
function Set-AutoEraseScheduleCore {
    [CmdletBinding()] param($CategoryId,$IntervalMinutes,$RunAsSystem,$TargetUser,[switch]$PreserveExisting,[switch]$ManagedByAutoSet,[switch]$AllowDefaultExcluded)
    if ($script:expectManaged -and (-not $PreserveExisting -or -not $ManagedByAutoSet)) { throw 'Managed creation did not force safety flags.' }
    $script:writes++
    $script:lastTarget = $TargetUser
    if ($TargetUser -eq 'OtherUser') { return @{ error=$true; message='Synthetic failure' } }
    @{ status='enabled'; categoryId=$CategoryId }
}
$script:expectManaged = $true

$manualUser = [pscustomobject]@{ Category='dnsCache'; Owner='S-1-5-21-1'; State='Disabled'; TaskName='manual-user' }
$managedSystem = [pscustomobject]@{ Category='dnsCache'; Owner='S-1-5-18'; State='Ready'; TaskName='system' }
$script:tasks = @($manualUser)
$result = Set-ManagedAutoEraseSchedule -CategoryId dnsCache -IntervalMinutes 60 -RunAsSystem $true
if ($result.status -ne 'alreadyConfigured' -or $result.taskState -ne 'Disabled' -or $script:writes -ne 0) { throw 'An existing disabled individual task was not preserved.' }
foreach ($order in @(0, 1)) {
    $script:tasks = if ($order -eq 0) { @($manualUser, $managedSystem) } else { @($managedSystem, $manualUser) }
    $result = Set-ManagedAutoEraseSchedule -CategoryId dnsCache -IntervalMinutes 60
    if (-not $result.error -or $script:writes -ne 0) { throw 'Duplicate local owners were not rejected.' }
}
$script:tasks = @([pscustomobject]@{ Category='dnsCache'; Owner='S-1-5-21-1'; State='Unknown'; TaskName='unknown' })
if (-not (Set-ManagedAutoEraseSchedule -CategoryId dnsCache -IntervalMinutes 60).error) { throw 'Unknown task state was accepted.' }
$script:tasks = @([pscustomobject]@{ Category='dnsCache'; Owner='S-1-5-18'; State='Ready'; TaskName='altered-system'; Altered=$true })
$result = Set-ManagedAutoEraseSchedule -CategoryId dnsCache -IntervalMinutes 60
if (-not $result.error -or $result.message -notlike '*Task Scheduler*' -or $script:writes -ne 0) { throw 'Altered system task did not block duplicate managed user creation.' }
$script:tasks = @([pscustomobject]@{ Category='dnsCache'; Owner='S-1-5-21-2'; State='Ready'; TaskName='altered-other'; Altered=$true })
$result = Set-ManagedAutoEraseSchedule -CategoryId dnsCache -IntervalMinutes 60
if ($result.status -ne 'enabled' -or $script:writes -ne 1) { throw 'An unrelated other-user task blocked the current user.' }
$script:writes = 0
$script:tasks = @()
$result = Set-ManagedAutoEraseSchedule -CategoryId dnsCache -IntervalMinutes 60
if ($result.status -ne 'enabled' -or $script:writes -ne 1) { throw 'New managed setup did not use the protected child operation.' }
$before = $script:writes
$bulkMulti = Set-MultiUserAutoEraseSchedule -CategoryId dnsCache -IntervalMinutes 60 -TargetUsers 'SyntheticUser,OtherUser' -ManagedByAutoSet
if (-not $bulkMulti.error -or $script:writes -ne $before) { throw 'Managed multi-user setup was not rejected before mutation.' }
$script:expectManaged = $false
$manualMulti = Set-MultiUserAutoEraseSchedule -CategoryId dnsCache -IntervalMinutes 60 -TargetUsers 'SyntheticUser,OtherUser'
if (-not $manualMulti.error -or $manualMulti.status -ne 'failed' -or $manualMulti.total -ne 2) { throw 'A partial multi-user failure was reported as success.' }
$script:profileMode = 'failed'
function Get-CimInstance {
    [CmdletBinding()] param([string]$ClassName)
    if ($script:profileMode -eq 'failed') { throw 'Synthetic profile access failure' }
    if ($script:profileMode -in @('present', 'unresolved')) { [pscustomobject]@{ Special=$false; SID='S-1-5-21-2'; LocalPath="$env:SystemDrive\Users\SyntheticUser" } }
}
function Resolve-AutoEraseProfileAccount {
    param([string]$Sid)
    if ($script:profileMode -eq 'unresolved') { throw 'Synthetic unresolved account' }
    if ($Sid -ne 'S-1-5-21-2') { throw 'Wrong profile SID' }
    'SYNTHETIC\RenamedAccount'
}
$before = $script:writes
$failedDiscovery = Set-MultiUserAutoEraseSchedule -CategoryId dnsCache -IntervalMinutes 60
if (-not $failedDiscovery.error -or $failedDiscovery.status -ne 'failed' -or $failedDiscovery.total -ne 0 -or $script:writes -ne $before) { throw 'Failed profile enumeration was reported as successful or created a task.' }
$script:profileMode = 'empty'
$noProfiles = Set-MultiUserAutoEraseSchedule -CategoryId dnsCache -IntervalMinutes 60
if (-not $noProfiles.error -or $noProfiles.status -ne 'failed' -or $noProfiles.total -ne 0 -or $script:writes -ne $before) { throw 'Empty profile enumeration was reported as successful or created a task.' }
$script:profileMode = 'present'
$oneProfile = Set-MultiUserAutoEraseSchedule -CategoryId dnsCache -IntervalMinutes 60
if ($oneProfile.error -or $oneProfile.total -ne 1 -or $script:writes -ne ($before + 1) -or $script:lastTarget -ne 'SYNTHETIC\RenamedAccount') { throw 'The discovered profile did not use its SID-resolved account.' }
$script:profileMode = 'unresolved'
$before = $script:writes
$unknownAccount = Set-MultiUserAutoEraseSchedule -CategoryId dnsCache -IntervalMinutes 60
if (-not $unknownAccount.error -or $unknownAccount.total -ne 0 -or $script:writes -ne $before) { throw 'An unresolved profile account was accepted.' }
$script:profileMode = 'failed'
$before = $script:writes
$system = Set-MultiUserAutoEraseSchedule -CategoryId dnsCache -IntervalMinutes 60 -RunAsSystem $true
if ($system.error -or $system.total -ne 1 -or $script:writes -ne ($before + 1)) { throw 'A system schedule incorrectly depended on profile enumeration.' }
Write-Output 'Passed: managed setup preserves both owner scopes, disabled tasks, and rejects ambiguous or unknown state.'
