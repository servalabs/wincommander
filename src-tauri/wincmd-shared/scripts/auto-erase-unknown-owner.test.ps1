$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')

function Assert-AutoEraseAdmin { }
function Invoke-AutoEraseScheduleLock { param($CategoryId, $Operation) & $Operation }
function Get-AutoEraseCurrentIdentity { @{ name='TEST\User'; sid='S-1-5-21-1-2-3-1001' } }
function Resolve-AutoEraseSid { param($Account) 'S-1-5-21-1-2-3-1001' }
function Get-AutoEraseTaskIdentity { param($Task, [switch]$AllowUnknownOwner) $Task.identity }
function Get-AutoEraseReadbackIdentity { param($Task, [switch]$AllowUnknownOwner) $null }
function Initialize-SystemMaintenanceTaskFolder { $script:folderWrites++ }
function Unregister-ScheduledTask { [CmdletBinding()] param($TaskPath, $TaskName, [switch]$Confirm) $script:removed += $TaskName }
function Get-ScheduledTask { [CmdletBinding()] param($TaskPath, $TaskName) $script:tasks }

$unknown = [pscustomobject]@{ TaskPath='\System Maintenance\'; TaskName='unknown'; identity=@{ categoryId='dnsCache'; ownerUnknown=$true; ownerSid=$null } }
$known = [pscustomobject]@{ TaskPath='\System Maintenance\'; TaskName='known'; identity=@{ categoryId='dnsCache'; ownerUnknown=$false; ownerSid='S-1-5-21-1-2-3-1001' } }
$other = [pscustomobject]@{ TaskPath='\System Maintenance\'; TaskName='other'; identity=@{ categoryId='psHistory'; ownerUnknown=$false; ownerSid='S-1-5-21-1-2-3-1001' } }
$script:tasks = @($known, $unknown, $other)
$script:removed = @(); $script:folderWrites = 0

$create = Set-AutoEraseScheduleCore -CategoryId dnsCache -IntervalMinutes 60
if (-not $create.error -or $script:folderWrites -ne 0) { throw 'An unknown owner must block same-category creation before task changes.' }
$migration = Invoke-AutoEraseMigrationCore -CategoryId dnsCache
if (-not $migration.error -or $script:folderWrites -ne 0) { throw 'An unknown owner must block same-category migration before task changes.' }
$remove = Remove-MultiUserAutoEraseScheduleCore -CategoryId dnsCache -TargetUsers 'TEST\User'
if (-not $remove.error -or $script:removed.Count -ne 0) { throw 'An unknown owner must block same-category removal before any task is removed.' }
$otherRemoval = Remove-MultiUserAutoEraseScheduleCore -CategoryId psHistory -TargetUsers 'TEST\User'
if ($otherRemoval.error -or @($script:removed).Count -ne 1 -or $script:removed[0] -ne 'other') { throw 'An unknown owner in another category must not block a verified removal.' }

# Verify the managed creation gate independently of registration side effects.
function Set-AutoEraseScheduleCore { param($CategoryId, $IntervalMinutes, $RunAsSystem, [switch]$PreserveExisting, [switch]$ManagedByAutoSet) @{ status='enabled'; categoryId=$CategoryId } }
$same = Set-ManagedAutoEraseSchedule -CategoryId dnsCache -IntervalMinutes 60
if (-not $same.error) { throw 'An unknown owner must block same-category Auto-set.' }
$script:tasks = @($unknown)
$unrelated = Set-ManagedAutoEraseSchedule -CategoryId psHistory -IntervalMinutes 60
if ($unrelated.error -or $unrelated.status -ne 'enabled') { throw 'An unknown owner in another category must not block Auto-set.' }
