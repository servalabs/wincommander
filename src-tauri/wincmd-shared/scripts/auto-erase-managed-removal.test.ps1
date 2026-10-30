$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')
foreach ($state in @('Ready','Running','Queued')) { if (-not (Test-AutoEraseTaskEnabled $state)) { throw "Active state rejected: $state" } }
foreach ($state in @('Disabled','Unknown','')) { if (Test-AutoEraseTaskEnabled $state) { throw "Unverified state reported active: $state" } }

# Every Windows task boundary below is mocked. No real task is read or changed.
function Assert-AutoEraseAdmin {}
function Invoke-AutoEraseScheduleLock { param($CategoryId,$Operation) & $Operation }
function Resolve-AutoEraseSid { param($Account) if ($Account -eq 'UnknownUser') { return '' }; $Account }
$script:tasks = @(
    [pscustomobject]@{ TaskPath='\SystemMaintenance\'; TaskName='managed'; Category='dnsCache'; Owner='S-1-5-18'; Description='System Maintenance scheduled cleanup v6 (managed)' },
    [pscustomobject]@{ TaskPath='\SystemMaintenance\'; TaskName='manual'; Category='dnsCache'; Owner='S-1-5-21-1'; Description='System Maintenance scheduled cleanup v6' },
    [pscustomobject]@{ TaskPath='\SystemMaintenance\'; TaskName='other'; Category='recentFiles'; Owner='S-1-5-18'; Description='System Maintenance scheduled cleanup v6 (managed)' }
)
$script:removed = @()
function Get-ScheduledTask { [CmdletBinding()] param() $script:tasks }
function Get-AutoEraseTaskIdentity { param($Task) @{ categoryId=$Task.Category; ownerSid=$Task.Owner } }
function Unregister-ScheduledTask { [CmdletBinding()] param($TaskPath,$TaskName,[switch]$Confirm) $script:removed += $TaskName }

$result = Remove-MultiUserAutoEraseSchedule -CategoryId dnsCache -TargetUsers ',, ,'
if (-not $result.error -or @($script:removed).Count -ne 0) { throw 'An invalid explicit target list must not remove every user schedule.' }
$result = Remove-MultiUserAutoEraseSchedule -CategoryId dnsCache -TargetUsers 'S-1-5-18,UnknownUser'
if (-not $result.error -or @($script:removed).Count -ne 0) { throw 'An unresolved target must prevent all removals.' }

$result = Remove-MultiUserAutoEraseSchedule -CategoryId dnsCache -TargetUsers 'S-1-5-18,S-1-5-21-1' -ManagedOnly
if ($result.error -or @($script:removed).Count -ne 1 -or $script:removed[0] -ne 'managed') { throw 'Managed removal affected a manual or unrelated task.' }
if (@($result.removedOwnerSids).Count -ne 1 -or $result.removedOwnerSids[0] -ne 'S-1-5-18') { throw 'Removed owner identities were not retained.' }
$scopeKeys = @(Get-AutoEraseRemovedScopeKeys -CurrentSid 'S-1-5-21-1' -CurrentUser 'SyntheticUser' -RemovedOwnerSids @($result.removedOwnerSids) -ManagedOnly)
if ($scopeKeys.Count -ne 1 -or $scopeKeys[0] -ne 'system') { throw 'Managed removal would alter the preserved user schedule state.' }
$scopeKeys = @(Get-AutoEraseRemovedScopeKeys -CurrentSid 'S-1-5-21-1' -CurrentUser 'SyntheticUser' -RemovedOwnerSids @() -ManagedOnly)
if ($scopeKeys.Count -ne 0) { throw 'A no-op managed removal would alter existing schedule state.' }

$script:removed = @()
$result = Remove-MultiUserAutoEraseSchedule -CategoryId dnsCache -TargetUsers 'S-1-5-18,S-1-5-21-1'
if ($result.error -or @($script:removed).Count -ne 2 -or 'manual' -notin $script:removed) { throw 'Individual removal behavior changed.' }

# Creation accepts the UI alias and writes the canonical clipboard category.
# Removal must resolve the same alias rather than silently leaving the task.
$script:tasks += [pscustomobject]@{ TaskPath='\SystemMaintenance\'; TaskName='clipboard-task'; Category='clipboard'; Owner='S-1-5-21-1'; Description='System Maintenance scheduled cleanup v6' }
$script:removed = @()
$result = Remove-MultiUserAutoEraseSchedule -CategoryId clipboardHistory -TargetUsers 'S-1-5-21-1'
if ($result.error -or $result.categoryId -ne 'clipboard' -or @($script:removed).Count -ne 1 -or $script:removed[0] -ne 'clipboard-task') { throw 'Clipboard alias did not remove the canonical schedule.' }

# A later failure must not hide the tasks that were already removed.
$script:removed = @()
function Unregister-ScheduledTask { [CmdletBinding()] param($TaskPath,$TaskName,[switch]$Confirm) if ($TaskName -eq 'manual') { throw 'Synthetic failure' }; $script:removed += $TaskName }
$result = Remove-MultiUserAutoEraseSchedule -CategoryId dnsCache -TargetUsers 'S-1-5-18,S-1-5-21-1'
if (-not $result.error -or $result.status -ne 'partial' -or @($result.removed).Count -ne 1 -or $result.removed[0] -ne 'managed' -or @($result.removedOwnerSids).Count -ne 1 -or $result.message -notmatch '1 removed before the error') { throw 'Partial removals were not reported accurately.' }

# Assert that the dedicated command cannot fall back to unqualified removal.
function Remove-AutoEraseSchedule { [CmdletBinding()] param($CategoryId,[switch]$ManagedOnly) @{ categoryId=$CategoryId; managed=[bool]$ManagedOnly } }
$result = Remove-ManagedAutoEraseSchedule -CategoryId dnsCache
if (-not $result.managed -or $result.categoryId -ne 'dnsCache') { throw 'Dedicated command did not require managed-only removal.' }
Write-Output 'Passed: managed removal preserved individual and unrelated tasks; individual removal remains available.'
