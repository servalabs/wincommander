$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')
function Assert-AutoEraseAdmin {}
function Assert-True($Value, [string]$Message) { if (-not $Value) { throw $Message } }
$script:tasks = @{}
$script:mutations = @()
$script:denyDelete = $false
$script:denyCreate = $false
$script:corruptReadback = $false
function New-Fixture([string]$Name, [string]$Sid = 'S-1-5-21-10-20-30-1001', [bool]$Enabled = $true) {
    $path = Join-Path $env:ProgramData 'WinCommander\auto-erase\scripts\clipboard.user-test.ps1'
    [xml]$xml = @"
<Task><RegistrationInfo><URI>\$Name</URI><Description>WinCommander Auto-set scheduled wipe v2</Description></RegistrationInfo><Triggers><TimeTrigger><StartBoundary>2025-01-01T12:30:00</StartBoundary><Repetition><Interval>PT17M</Interval><Duration>P9999D</Duration></Repetition></TimeTrigger></Triggers><Principals><Principal><UserId>$Sid</UserId><LogonType>S4U</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals><Settings><Enabled>$($Enabled.ToString().ToLowerInvariant())</Enabled><StartWhenAvailable>true</StartWhenAvailable></Settings><Actions><Exec><Command>powershell.exe</Command><Arguments>-NoProfile -File &quot;$path&quot;</Arguments></Exec></Actions></Task>
"@
    Set-Fixture $Name $xml.OuterXml
}
function Set-Fixture([string]$Name, [string]$Xml) {
    [xml]$x = $Xml
    $script:tasks[$Name] = [pscustomobject]@{
        TaskName = $Name; TaskPath = '\'; Description = [string]$x.Task.RegistrationInfo.Description
        State = if ($x.Task.Settings.Enabled -eq 'false') { 'Disabled' } else { 'Ready' }
        Principal = [pscustomobject]@{ UserId = [string]$x.Task.Principals.Principal.UserId }
        Actions = @([pscustomobject]@{ Execute = [string]$x.Task.Actions.Exec.Command; Arguments = [string]$x.Task.Actions.Exec.Arguments })
        Triggers = @([pscustomobject]@{ Repetition = [pscustomobject]@{ Interval = 'PT17M' } })
        Xml = $Xml
    }
}
function Get-ScheduledTask { param($TaskName, $TaskPath, $ErrorAction)
    if ($TaskName) { return $script:tasks[$TaskName] }
    @($script:tasks.Values)
}
function Export-ScheduledTask { param($TaskName, $TaskPath, $ErrorAction) $script:tasks[$TaskName].Xml }
function Register-ScheduledTask { param($TaskName, $TaskPath, $Xml, [switch]$Force, $ErrorAction)
    if ($script:denyCreate) { throw 'simulated register denied' }
    $script:mutations += 'register:' + $TaskName
    Set-Fixture $TaskName $Xml
    if ($script:corruptReadback -and $TaskName.StartsWith('SL-')) {
        [xml]$changed = $script:tasks[$TaskName].Xml
        $changed.Task.Principals.Principal.UserId = 'S-1-5-18'
        Set-Fixture $TaskName $changed.OuterXml
    }
}
function Unregister-ScheduledTask { param($TaskName, $TaskPath, $Confirm, $ErrorAction)
    if ($script:denyDelete -and $TaskName.StartsWith('WinCommander')) { throw 'simulated delete denied' }
    $script:mutations += 'delete:' + $TaskName
    $script:tasks.Remove($TaskName)
}
function Enable-ScheduledTask { param($TaskName, $TaskPath, $ErrorAction)
    $script:mutations += 'enable:' + $TaskName
    [xml]$x = $script:tasks[$TaskName].Xml
    $x.Task.Settings.Enabled = 'true'
    Set-Fixture $TaskName $x.OuterXml
}
function Get-ScheduledTaskInfo { param($TaskName, $TaskPath, $ErrorAction)
    [pscustomobject]@{ LastRunTime = [datetime]'2025-01-01'; NextRunTime = [datetime]'2025-01-02'; LastTaskResult = 0 }
}
function Start-ScheduledTask { throw 'Migration must never execute a cleanup task' }
function ConvertTo-AutoEraseTaskArgument { throw 'Current payload must not be rewritten during rename' }
function ConvertTo-AutoEraseComparableTaskXml { param([xml]$Document) return $Document }

$legacy = 'WinCommander_AutoErase_clipboard'
$sid = 'S-1-5-21-10-20-30-1001'
$newName = Get-AutoEraseTaskName 'clipboard' $sid
Assert-True ($newName -match '^SL-UW-[A-F0-9]{8}-S-1-') 'Expected coded per-user name'
Assert-True ((Get-AutoEraseTaskName 'clipboard' 'S-1-5-18') -match '^SL-SW-[A-F0-9]{8}$') 'Expected distinct SYSTEM name'
Assert-True ($newName -ne (Get-AutoEraseTaskName 'clipboard' 'S-1-5-21-10-20-30-1002')) 'Users must have distinct task names'
New-Fixture $legacy $sid $false
$before = $script:tasks[$legacy].Xml
$result = Invoke-AutoEraseMigration
Assert-True (-not $result.error) 'Disabled task migration failed'
Assert-True (-not $script:tasks.ContainsKey($legacy)) 'Legacy task was left behind'
Assert-True ($script:tasks[$newName].State -eq 'Disabled') 'Disabled schedule was enabled'
[xml]$oldXml = $before
[xml]$newXml = $script:tasks[$newName].Xml
foreach ($part in @('Triggers', 'Principals', 'Actions', 'Settings')) {
    Assert-True ($oldXml.Task.$part.OuterXml -eq $newXml.Task.$part.OuterXml) "Migration changed $part"
}
$count = $script:mutations.Count
$result = Invoke-AutoEraseMigration
Assert-True ($script:mutations.Count -eq $count) 'Repeat migration must be a no-op'
$listed = Get-AutoEraseSchedules
Assert-True ($listed.total -eq 1 -and $listed.schedules[0].intervalMinutes -eq 17) 'Coded names must remain listable'

$script:tasks = @{}; $script:mutations = @()
New-Fixture $legacy $sid $true
$result = Invoke-AutoEraseMigration
Assert-True (-not $result.error -and $script:tasks[$newName].State -eq 'Ready') 'Enabled state was not restored'
Assert-True ($script:mutations[-1] -eq "enable:$newName") 'Replacement must only enable after original deletion'

foreach ($omit in @('Enabled', 'Settings')) {
    $script:tasks = @{}; $script:mutations = @()
    New-Fixture $legacy $sid $true
    [xml]$sparse = $script:tasks[$legacy].Xml.Replace('<Task>', '<Task xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">')
    if ($omit -eq 'Enabled') { [void]$sparse.Task.Settings.RemoveChild($sparse.Task.Settings.SelectSingleNode('*[local-name()="Enabled"]')) }
    else { [void]$sparse.Task.RemoveChild($sparse.Task.SelectSingleNode('*[local-name()="Settings"]')) }
    Set-Fixture $legacy $sparse.OuterXml
    $result = Invoke-AutoEraseMigration
    Assert-True (-not $result.error) "Default-enabled task with omitted $omit must migrate: $($result.message)"
    Assert-True ($script:tasks[$newName].State -eq 'Ready') "Omitted $omit must retain the Windows enabled default"
    [xml]$migrated = $script:tasks[$newName].Xml
    Assert-True ($migrated.Task.Settings.NamespaceURI -eq $migrated.Task.NamespaceURI) 'Created settings must use the task namespace'
    Assert-True ($migrated.Task.Settings.SelectSingleNode('*[local-name()="Enabled"]').NamespaceURI -eq $migrated.Task.NamespaceURI) 'Created Enabled must use the task namespace'
}

$script:tasks = @{}; $script:mutations = @()
New-Fixture $legacy
$script:denyCreate = $true
$result = Invoke-AutoEraseMigration
Assert-True ($result.error -and $script:tasks.ContainsKey($legacy)) 'Registration failure must preserve original'
$script:denyCreate = $false; $script:denyDelete = $true
$result = Invoke-AutoEraseMigration
Assert-True ($result.error -and $script:tasks.ContainsKey($legacy) -and -not $script:tasks.ContainsKey($newName)) 'Delete failure must roll back staged replacement'
$script:denyDelete = $false

$script:corruptReadback = $true
$result = Invoke-AutoEraseMigration
Assert-True ($result.error -and $script:tasks.ContainsKey($legacy) -and -not $script:tasks.ContainsKey($newName)) 'A changed principal on readback must roll back migration'
$script:corruptReadback = $false
$script:tasks[$legacy].State = 'Running'
$count = $script:mutations.Count
$result = Invoke-AutoEraseMigration
Assert-True ($result.error -and $script:mutations.Count -eq $count) 'A running wipe must not be stopped or duplicated'

$script:tasks = @{}; $script:mutations = @()
New-Fixture $legacy
New-Fixture $newName
$result = Invoke-AutoEraseMigration
Assert-True ($result.error -and $script:mutations.Count -eq 0 -and $script:tasks.Count -eq 2) 'Occupied replacement must not overwrite either task'

$script:tasks = @{}; $script:mutations = @()
New-Fixture $legacy
$script:tasks[$legacy].Actions[0].Arguments = '-File "C:\Unrelated\clipboard.ps1"'
$result = Invoke-AutoEraseMigration
Assert-True ($script:mutations.Count -eq 0) 'An unrelated same-name task must not migrate'
$result = Remove-MultiUserAutoEraseSchedule -CategoryId clipboard
Assert-True ($script:tasks.Count -eq 1) 'An unrelated same-name task must not be deleted'

$script:tasks = @{}; $script:mutations = @()
New-Fixture $legacy
New-Fixture 'System_AutoErase_clipboard_other' 'S-1-5-21-10-20-30-1002'
New-Fixture 'WinCommander_AutoErase_clipboard_system' 'S-1-5-18'
$result = Invoke-AutoEraseMigration
Assert-True (-not $result.error -and $script:tasks.Count -eq 3) 'Per-user and SYSTEM schedules must coexist'
$result = Remove-MultiUserAutoEraseSchedule -CategoryId clipboard -TargetUsers $sid
Assert-True ($script:tasks.Count -eq 2 -and -not $script:tasks.ContainsKey($newName)) 'Selected-user removal must preserve other owners'
Write-Output 'PASS: coded identity, ownership, exact schedule migration, idempotency, collision and rollback behavior'
