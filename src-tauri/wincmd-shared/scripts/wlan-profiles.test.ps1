$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')
. (Join-Path $PSScriptRoot '../../commander-free/scripts/modules/privacy/cleanup.ps1')

$adapterA = '00000000-0000-0000-0000-000000000001'
$adapterB = '00000000-0000-0000-0000-000000000002'
function Reset-Fixture {
    $script:adapters = @($adapterA, $adapterB)
    $script:profiles = @(
        [pscustomobject]@{ InterfaceId=$adapterA; Name="Cafe's network"; Flags=0 },
        [pscustomobject]@{ InterfaceId=$adapterB; Name='Work'; Flags=0 },
        [pscustomobject]@{ InterfaceId=$adapterA; Name='Private'; Flags=2 },
        [pscustomobject]@{ InterfaceId=$adapterB; Name='Policy'; Flags=1 }
    )
    $script:removed = @(); $script:reads=0; $script:failReadAt=0
    $script:failDelete=$false; $script:removeAdapterAfterDelete=$false; $script:addAfterDelete=$false
}
function Get-WCWlanSnapshot {
    $script:reads++
    if ($script:reads -eq $script:failReadAt) { throw 'Synthetic enumeration failure' }
    @{ InterfaceCount=$script:adapters.Count; Interfaces=@($script:adapters); Profiles=@($script:profiles) }
}
function Remove-WCWlanEntry {
    param([string]$InterfaceId,[string]$Name)
    if ($script:failDelete) { throw 'Synthetic deletion failure' }
    $script:removed += "$InterfaceId|$Name"
    $script:profiles = @($script:profiles | Where-Object { $_.InterfaceId -ine $InterfaceId -or $_.Name -cne $Name })
    if ($script:removeAdapterAfterDelete) { $script:adapters=@($script:adapters | Where-Object { $_ -ine $InterfaceId }) }
    if ($script:addAfterDelete) { $script:profiles += [pscustomobject]@{ InterfaceId=$adapterB; Name='New profile'; Flags=0 }; $script:addAfterDelete=$false }
}
function Get-WCWlanKey {
    param([string]$InterfaceId,[string]$Name)
    $script:keyReads += "$InterfaceId|$Name"
    if ($InterfaceId -eq $adapterB) { throw 'Synthetic access denied' }
    return 'synthetic-password'
}
function Check($Condition,[string]$Message) { if (-not $Condition) { throw $Message } }

Reset-Fixture
$script:keyReads = @()
$scan = Get-WlanProfiles -IncludeSecrets:$false
Check ($scan.total -eq 2 -and @($scan.profiles).Count -eq 2) 'Scanner did not retain the all-user scope.'
Check ($scan.coverage -eq 'limited') 'Excluded private and managed profiles must remain a limited scan.'
Check (-not ($scan | ConvertTo-Json -Depth 5).Contains('password')) 'Scanner returned credentials.'
Check ($script:keyReads.Count -eq 0) 'Normal scan requested a credential.'
$script:profiles += [pscustomobject]@{ InterfaceId=$adapterB; Name="Cafe's network"; Flags=0 }
$revealed = Get-WlanProfiles -IncludeSecrets:$true
$first = @($revealed.profiles | Where-Object { $_.name -ceq "Cafe's network" -and $_.interfaceId -eq $adapterA })[0]
$second = @($revealed.profiles | Where-Object { $_.name -ceq "Cafe's network" -and $_.interfaceId -eq $adapterB })[0]
Check ($first.password -eq 'synthetic-password' -and $first.passwordStatus -eq 'available') 'Explicit reveal failed for an available key.'
Check ($null -eq $second.password -and $second.passwordStatus -eq 'unavailable') 'Denied duplicate profile exposed or reused another adapter key.'
Check ($script:keyReads.Count -eq 3) 'Explicit reveal did not inspect each all-user adapter profile.'
Reset-Fixture
$result = Invoke-WCWlanClear
Check ($result.ok -and $result.removed -eq 2 -and $result.failed -eq 0) 'All-user profiles were not verified removed.'
Check ($script:removed.Count -eq 2 -and @($script:profiles).Count -eq 2) 'Private or policy profiles were touched.'
Check (-not $result.stdout.Contains("Cafe's network")) 'Receipt exposed a profile name.'

Reset-Fixture
$result = Invoke-WCWlanClear -Name "Cafe's network"
Check ($result.ok -and $result.removed -eq 1 -and @($script:profiles | Where-Object Name -eq 'Work').Count -eq 1) 'Named clear widened its scope.'

Reset-Fixture
$script:profiles += [pscustomobject]@{ InterfaceId=$adapterB; Name="Cafe's network"; Flags=0 }
$result = Invoke-WCWlanClear -Name "Cafe's network"
Check (-not $result.ok -and $script:removed.Count -eq 0) 'A name shared by adapters must not delete multiple profiles.'

Reset-Fixture
$result = Invoke-WCWlanClear -Name 'work'
Check (-not $result.ok -and $script:removed.Count -eq 0) 'Case-sensitive Wi-Fi profile identity was widened.'

Reset-Fixture
$script:profiles += [pscustomobject]@{ InterfaceId=$adapterA; Name="Cafe's network"; Flags=2 }
$result = Invoke-WCWlanClear -Name "Cafe's network"
Check (-not $result.ok -and $script:removed.Count -eq 0) 'Ambiguous profile identity was deleted.'

Reset-Fixture
$script:failReadAt=1
$result = Invoke-WCWlanClear
Check (-not $result.ok -and $script:removed.Count -eq 0) 'Enumeration failure allowed deletion.'

Reset-Fixture
$script:profiles += [pscustomobject]@{ InterfaceId=$adapterA; Name='Unknown profile type'; Flags=4 }
$result = Invoke-WCWlanClear
Check (-not $result.ok -and $script:removed.Count -eq 0) 'An unknown profile type allowed deletion.'

Reset-Fixture
$script:adapters=@($adapterA); $script:profiles=@()
$result = Invoke-WCWlanClear
Check ($result.ok -and $result.removed -eq 0 -and $result.failed -eq 0) 'A confirmed empty profile set was not handled correctly.'

Reset-Fixture
$script:failDelete=$true
$result = Invoke-WCWlanClear
Check (-not $result.ok -and $result.failed -gt 0) 'Deletion failure was reported as success.'

Reset-Fixture
$script:removeAdapterAfterDelete=$true
$result = Invoke-WCWlanClear -Name "Cafe's network"
Check (-not $result.ok) 'A disappearing adapter was treated as verified removal.'

Reset-Fixture
$script:addAfterDelete=$true
$result = Invoke-WCWlanClear
Check (-not $result.ok) 'A new profile after enumeration was falsely reported as cleared.'

Reset-Fixture
$script:adapters=@(); $script:profiles=@()
$scan = Get-WlanProfiles
Check ($scan.error -eq $true -and $null -eq $scan.profiles) 'Unavailable adapter was reported as an empty scan.'

function Assert-AutoEraseAdmin {}
function Initialize-SystemMaintenanceTaskFolder {}
function Get-ScheduledTask {
    [CmdletBinding()] param($TaskPath,$TaskName)
    if ($TaskName) { return [pscustomobject]@{ State='Disabled'; TaskName=$TaskName; TaskPath=$TaskPath } }
    @([pscustomobject]@{ TaskName='OldWifi'; TaskPath='\System Maintenance\'; Description='Legacy automatic cleanup' },
      [pscustomobject]@{ TaskName='Other'; TaskPath='\System Maintenance\'; Description='Unrelated fixture' })
}
function Get-AutoEraseTaskIdentity {
    param($Task)
    if ($Task.TaskName -eq 'OldWifi') { return @{ categoryId='wlanProfiles'; ownerSid='S-1-5-18'; taskName='OldWifi' } }
    @{ categoryId='dnsCache'; ownerSid='S-1-5-18'; taskName='Other' }
}
function Disable-ScheduledTask {
    [CmdletBinding()] param($TaskPath,$TaskName)
    $script:disabledTasks += $TaskName
}
$script:disabledTasks=@()
$migration = Invoke-AutoEraseMigration -CategoryId 'wlanProfiles'
Check (-not $migration.error -and $script:disabledTasks.Count -eq 1 -and $script:disabledTasks[0] -eq 'OldWifi') 'Unapproved legacy Wi-Fi task was not isolated and disabled.'
function Invoke-AutoEraseMigration { param($CategoryId) @{ status='ok' } }
function Get-ScheduledTask { [CmdletBinding()] param($TaskPath,$TaskName) if ($TaskName) { return $script:savedTask } }
function Get-AutoEraseTaskIdentity { param($Task) @{ categoryId='wlanProfiles'; ownerSid='S-1-5-18' } }
function ConvertTo-AutoEraseTaskArgument {
    param($CategoryId,$IntervalMinutes,$Script,$ExecutionScope,$FirstDueUtc,$AllowDefaultExcluded)
    $script:taskApproval=$AllowDefaultExcluded; $script:taskScope=$ExecutionScope; $script:taskBody=$Script
    'mock-task-argument'
}
function New-ScheduledTaskAction { param($Execute,$Argument) @{ execute=$Execute; argument=$Argument } }
function New-ScheduledTaskTrigger { param([switch]$Once,$At,$RepetitionInterval,$RepetitionDuration,[switch]$AtStartup,[switch]$AtLogOn,$User) @{ startup=[bool]$AtStartup; interval=$RepetitionInterval } }
function New-ScheduledTaskSettingsSet { param([switch]$AllowStartIfOnBatteries,[switch]$DontStopIfGoingOnBatteries,[switch]$StartWhenAvailable,[switch]$Hidden) @{} }
function New-ScheduledTaskPrincipal { param($UserId,$LogonType,$RunLevel) @{ user=$UserId; logon=$LogonType; level=$RunLevel } }
function Register-ScheduledTask {
    [CmdletBinding()] param($TaskPath,$TaskName,$Action,$Trigger,$Principal,$Settings,$Description,[switch]$Force)
    $script:registrations += @{ principal=$Principal; description=$Description; triggers=$Trigger }
    $script:savedTask = [pscustomobject]@{ TaskPath=$TaskPath; TaskName=$TaskName; Actions=@([pscustomobject]@{ Execute=$Action.execute; Arguments=$Action.argument }); Triggers=@([pscustomobject]@{ Repetition=[pscustomobject]@{ Interval=[Xml.XmlConvert]::ToString($Trigger[0].interval) } }); Description=$Description; State='Ready' }
}
$script:registrations=@()
$catalog = Get-AutoEraseSupportedCategories
Check ('wlanProfiles' -in $catalog.schedulableCategories -and 'wlanProfiles' -notin $catalog.categories) 'Wi-Fi manual and Auto-set catalog policy disagrees.'
Check ((Set-AutoEraseSchedule -CategoryId 'wlanProfiles' -IntervalMinutes 60).error) 'Protected Wi-Fi schedule accepted without individual consent.'
Check ((Set-AutoEraseSchedule -CategoryId 'wlanProfiles' -IntervalMinutes 60 -AllowDefaultExcluded -ManagedByAutoSet).error) 'Auto-set accepted protected Wi-Fi.'
Check ($script:registrations.Count -eq 0) 'Rejected requests reached registration.'
$registered = Set-AutoEraseSchedule -CategoryId 'wlanProfiles' -IntervalMinutes 60 -AllowDefaultExcluded
Check ($registered.status -eq 'enabled' -and $script:registrations.Count -eq 1) "Individual schedule did not reach mocked registration: $($registered.message)"
Check ($script:taskApproval -and $script:taskScope -eq 'system' -and $script:registrations[0].principal.user -eq 'SYSTEM') 'Explicit consent or system identity was lost.'
Check ($script:registrations[0].description -like '*(manual opt-in)' -and $script:taskBody.Contains('Invoke-WCWlanClear')) 'Task omitted its protected adapter.'
Check (-not $script:taskBody.Contains('key=clear') -and -not $script:taskBody.Contains('Wlansvc\Profiles\Interfaces')) 'Task included credential collection or raw service-file deletion.'

Write-Output 'Passed: synthetic WLAN scan, opt-in scope, errors, identity changes, privacy and readback.'
