$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')
$oldProgramData = $env:ProgramData
$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ('wc-payload-readback-' + [guid]::NewGuid())
try {
    $env:ProgramData = $tempRoot
    $scripts = Join-Path $tempRoot 'WinCommander\auto-erase\scripts'
    New-Item -ItemType Directory -Path $scripts -Force | Out-Null
    $path = Join-Path $scripts 'dnsCache.user-fixture.ps1'
    $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $taskName = Get-AutoEraseTaskName 'dnsCache' $sid
    $script:fixtureTask = [pscustomobject]@{
        TaskPath = $script:SystemMaintenanceTaskPath; TaskName = $taskName; State = 'Ready'
        Principal = [pscustomobject]@{ UserId = $sid; LogonType = 'S4U'; RunLevel = 'Highest' }
        Settings = [pscustomobject]@{ DisallowStartIfOnBatteries = $false; StopIfGoingOnBatteries = $false; RunOnlyIfIdle = $false; RunOnlyIfNetworkAvailable = $false; StartWhenAvailable = $true }
        Actions = @([pscustomobject]@{ Execute = 'powershell.exe'; Arguments = "-NoProfile -File `"$path`"" })
        Triggers = @(
            [pscustomobject]@{ Repetition = [pscustomobject]@{ Interval = 'PT60M'; Duration = 'P9999D' }; EndBoundary = ''; Enabled = $true; CimClass = [pscustomobject]@{ CimClassName = 'MSFT_TaskTimeTrigger' } },
            [pscustomobject]@{ Enabled = $true; UserId = $sid; EndBoundary = ''; CimClass = [pscustomobject]@{ CimClassName = 'MSFT_TaskLogonTrigger' } }
        )
        Description = 'System Maintenance scheduled cleanup v20'
    }
    function Get-ScheduledTask { [CmdletBinding()] param() $script:fixtureTask }
    function Get-ScheduledTaskInfo { [CmdletBinding()] param($TaskPath,$TaskName) $null }
    $missing = Get-AutoEraseSchedules
    if ($missing.error -or $missing.total -ne 1 -or -not $missing.schedules[0].enabled -or $missing.schedules[0].payloadVerified) { throw 'A Ready task with a missing payload must report its registration while its payload is unverified.' }
    [IO.File]::WriteAllText($path, "`$categoryId = 'dnsCache'`n")
    $truncated = Get-AutoEraseSchedules
    if ($truncated.error -or $truncated.schedules[0].payloadVerified) { throw 'A category line without cleanup code must not be verified.' }
    [IO.File]::WriteAllText($path, "`$categoryId = 'other'`n")
    $wrong = Get-AutoEraseSchedules
    if ($wrong.error -or $wrong.schedules[0].payloadVerified) { throw 'A mismatched payload must not be verified.' }
    $script:fixtureScripts = $scripts
    function Initialize-AutoEraseStorage { @{ scripts = $script:fixtureScripts } }
    $firstDue = [DateTime]::UtcNow.AddHours(1)
    $argument = ConvertTo-AutoEraseTaskArgument -CategoryId 'dnsCache' -IntervalMinutes 60 -Script $script:AutoEraseScripts['dnsCache'] -ExecutionScope 'user-fixture' -FirstDueUtc ($firstDue.ToString('o'))
    $script:fixtureTask.Actions[0].Arguments = $argument
    $script:fixtureTask.Triggers[0] | Add-Member -NotePropertyName StartBoundary -NotePropertyValue ($firstDue.ToString('o'))
    $present = Get-AutoEraseSchedules
    if ($present.error -or -not $present.schedules[0].payloadVerified) { throw 'A generated cleanup payload should be verified.' }
    # A valid script path does not prove the action will run it: -Command can
    # alter PowerShell's invocation even when the -File argument remains.
    $script:fixtureTask.Actions[0].Arguments = '-Command "Write-Output skipped" ' + $argument
    $alteredArguments = Get-AutoEraseSchedules
    if ($alteredArguments.error -or $alteredArguments.total -ne 1 -or $alteredArguments.schedules[0].payloadVerified) { throw 'Extra PowerShell arguments must not be verified as the generated action.' }
    $script:fixtureTask.Actions[0].Arguments = $argument
    $knownTask = $script:fixtureTask
    $unresolvedTask = [pscustomobject]@{
        TaskPath = $knownTask.TaskPath; TaskName = $knownTask.TaskName; State = 'Ready'
        Principal = [pscustomobject]@{ UserId = 'WC_UNRESOLVABLE_DOMAIN\MissingCleanupOwner'; LogonType = 'S4U'; RunLevel = 'Highest' }
        Settings = $knownTask.Settings; Actions = $knownTask.Actions; Triggers = $knownTask.Triggers; Description = $knownTask.Description
    }
    $script:fixtureTask = @($knownTask, $unresolvedTask)
    $unknown = Get-AutoEraseSchedules
    $unknownRows = @($unknown.schedules | Where-Object { $_.scope -eq 'unknown' })
    if ($unknown.error -or $unknown.total -ne 2 -or $unknownRows.Count -ne 1 -or $unknownRows[0].payloadVerified -or
        -not $unknownRows[0].requiresTaskSchedulerRepair -or $null -ne $unknownRows[0].targetUser -or $null -ne $unknownRows[0].cleanupOutcome) {
        throw 'An attributable task with an unresolved owner must remain visible as unknown and require Scheduler review.'
    }
    $strictFailed = $false
    try { $null = Get-AutoEraseTaskIdentity $unresolvedTask } catch { $strictFailed = $true }
    if (-not $strictFailed) { throw 'An unresolved owner must not acquire mutation identity.' }
    $unresolvedTask.Actions += [pscustomobject]@{ Execute = 'cmd.exe'; Arguments = '/c exit' }
    $alteredUnknown = Get-AutoEraseSchedules
    $alteredUnknownRows = @($alteredUnknown.schedules | Where-Object { $_.scope -eq 'unknown' })
    if ($alteredUnknown.error -or $alteredUnknownRows.Count -ne 1 -or -not $alteredUnknownRows[0].requiresTaskSchedulerRepair) { throw 'An altered cleanup task with unknown owner must remain visible for review.' }
    $blocked = $false
    try { Assert-AutoEraseKnownOwners -Tasks @($unresolvedTask) -CategoryId 'dnsCache' } catch { $blocked = $true }
    if (-not $blocked) { throw 'An altered cleanup task with unknown owner must block mutations in its category.' }
    $unresolvedTask.Actions = @([pscustomobject]@{ Execute = 'powershell.exe'; Arguments = '-File "C:\unrelated.ps1"' })
    $unrelated = Get-AutoEraseSchedules
    if ($unrelated.error -or $unrelated.total -ne 1) { throw 'An unrelated task with an unresolved owner must not be attributed to cleanup.' }
    $script:fixtureTask = $knownTask
    foreach ($invalidInterval in @('not-an-interval', 'P999999999D', '')) {
        $script:fixtureTask.Triggers[0].Repetition.Interval = $invalidInterval
        $badInterval = Get-AutoEraseSchedules
        if ($badInterval.error -or $badInterval.total -ne 1 -or -not $badInterval.schedules[0].enabled -or $badInterval.schedules[0].intervalMinutes -ne 0 -or $badInterval.schedules[0].payloadVerified) {
            throw 'An unreadable interval must leave the task listed as enabled and unverified.'
        }
    }
    $script:fixtureTask.Triggers[0].Repetition.Interval = 'PT60M'
    $script:fixtureTask.Triggers[0].StartBoundary = $firstDue.AddMinutes(-120).ToString('o')
    if (-not (Get-AutoEraseSchedules).schedules[0].payloadVerified) { throw 'A migrated payload aligned with an earlier trigger slot must be verified.' }
    $script:fixtureTask.Triggers[0].StartBoundary = $firstDue.AddMinutes(15).ToString('o')
    if ((Get-AutoEraseSchedules).schedules[0].payloadVerified) { throw 'A shifted trigger phase must not be verified.' }
    $script:fixtureTask.Triggers[0].StartBoundary = $firstDue.AddSeconds(-1).ToString('o')
    if ((Get-AutoEraseSchedules).schedules[0].payloadVerified) { throw 'Even a one-second early trigger can skip a cleanup and must not be verified.' }
    $script:fixtureTask.Triggers[0].StartBoundary = $firstDue.ToString('o')
    foreach ($field in @('DisallowStartIfOnBatteries', 'StopIfGoingOnBatteries', 'RunOnlyIfIdle', 'RunOnlyIfNetworkAvailable')) {
        $script:fixtureTask.Settings.$field = $true
        if ((Get-AutoEraseSchedules).schedules[0].payloadVerified) { throw "A task restricted by $field must not be verified." }
        $script:fixtureTask.Settings.$field = $false
    }
    $script:fixtureTask.Settings.StartWhenAvailable = $false
    if ((Get-AutoEraseSchedules).schedules[0].payloadVerified) { throw 'A task that cannot catch up must not be verified.' }
    $script:fixtureTask.Settings.StartWhenAvailable = $true
    $script:fixtureTask.Actions[0].Execute = 'cmd.exe'
    $wrongExecutable = Get-AutoEraseSchedules
    if ($wrongExecutable.error -or $wrongExecutable.total -ne 1 -or $wrongExecutable.schedules[0].payloadVerified -or -not $wrongExecutable.schedules[0].requiresTaskSchedulerRepair) { throw 'An edited executable must remain visible as unverified and require Scheduler repair.' }
    if (Get-AutoEraseTaskIdentity $script:fixtureTask) { throw 'Readback recognition must not relax mutation ownership.' }
    $script:fixtureTask.Actions[0].Arguments = '-c unrelated'
    if ((Get-AutoEraseSchedules).total -ne 0) { throw 'An unrelated action must not be identified as cleanup.' }
    $script:fixtureTask.Actions[0].Arguments = $argument
    $script:fixtureTask.Actions[0].Execute = 'powershell.exe'
    $script:fixtureTask.Actions += [pscustomobject]@{ Execute = 'cmd.exe'; Arguments = '/c exit' }
    $extraAction = Get-AutoEraseSchedules
    if ($extraAction.error -or $extraAction.total -ne 1 -or $extraAction.schedules[0].payloadVerified -or -not $extraAction.schedules[0].requiresTaskSchedulerRepair) { throw 'An extra action must remain visible as unverified and require Scheduler repair.' }
    if (Get-AutoEraseTaskIdentity $script:fixtureTask) { throw 'Additional actions must not become eligible for in-app mutation.' }
    $script:fixtureTask.Actions = @($script:fixtureTask.Actions[0])
    $script:fixtureTask.Triggers[0].Enabled = $false
    $disabledTime = Get-AutoEraseSchedules
    if ($disabledTime.error -or $disabledTime.schedules[0].payloadVerified) { throw 'A disabled repeating trigger must not be verified.' }
    $script:fixtureTask.Triggers[0].Enabled = $true
    $script:fixtureTask.Triggers[0].Repetition.Duration = 'PT1H'
    if ((Get-AutoEraseSchedules).schedules[0].payloadVerified) { throw 'A repeating trigger with a short duration must not be verified.' }
    $script:fixtureTask.Triggers[0].Repetition.Duration = 'not-a-duration'
    if ((Get-AutoEraseSchedules).schedules[0].payloadVerified) { throw 'An invalid repetition duration must not be verified.' }
    $script:fixtureTask.Triggers[0].Repetition.Duration = 'P9999D'
    $script:fixtureTask.Triggers[0].EndBoundary = '2030-01-01T00:00:00'
    if ((Get-AutoEraseSchedules).schedules[0].payloadVerified) { throw 'A repeating trigger with an end boundary must not be verified.' }
    $script:fixtureTask.Triggers[0].EndBoundary = ''
    $script:fixtureTask.Triggers[1].EndBoundary = '2030-01-01T00:00:00'
    if ((Get-AutoEraseSchedules).schedules[0].payloadVerified) { throw 'A catch-up trigger with an end boundary must not be verified.' }
    $script:fixtureTask.Triggers[1].EndBoundary = ''
    $script:fixtureTask.Triggers[1].Enabled = $false
    $disabledLogon = Get-AutoEraseSchedules
    if ($disabledLogon.error -or $disabledLogon.schedules[0].payloadVerified) { throw 'A disabled catch-up trigger must not be verified.' }
    $script:fixtureTask.Triggers[1].Enabled = $true
    $script:fixtureTask.Triggers[1].UserId = 'S-1-5-18'
    $wrongLogonOwner = Get-AutoEraseSchedules
    if ($wrongLogonOwner.error -or $wrongLogonOwner.schedules[0].payloadVerified) { throw 'A catch-up trigger for another account must not be verified.' }
    $script:fixtureTask.Triggers[1].UserId = $sid
    $script:fixtureTask.Principal.LogonType = 'Interactive'
    $wrongLogon = Get-AutoEraseSchedules
    if ($wrongLogon.error -or $wrongLogon.schedules[0].payloadVerified) { throw 'An incompatible Scheduler logon type must not be verified.' }
    $script:fixtureTask.Principal.LogonType = 'S4U'
    $script:fixtureTask.Principal.RunLevel = 'Limited'
    $wrongLevel = Get-AutoEraseSchedules
    if ($wrongLevel.error -or $wrongLevel.schedules[0].payloadVerified) { throw 'A user task with insufficient privilege must not be verified.' }
    $script:fixtureTask.Principal.RunLevel = 'Highest'
    $script:fixtureTask.Triggers[0].Repetition.Interval = 'PT30M'
    $changedInterval = Get-AutoEraseSchedules
    if ($changedInterval.error -or $changedInterval.schedules[0].payloadVerified) { throw 'A Scheduler interval that differs from the payload cadence must not be verified.' }
    $script:fixtureTask.Triggers[0].Repetition.Interval = 'PT60M30S'
    $fractionalInterval = Get-AutoEraseSchedules
    if ($fractionalInterval.error -or $fractionalInterval.schedules[0].payloadVerified) { throw 'A fractional-minute Scheduler interval must not be rounded into a verified cadence.' }
    $script:fixtureTask.Triggers[0].Repetition.Interval = 'PT60M'
    if ($argument -notmatch '-File\s+"([^"]+)"') { throw 'Fixture payload path missing.' }
    $generatedPath = $Matches[1]
    $generated = [IO.File]::ReadAllText($generatedPath)
    [IO.File]::WriteAllText($generatedPath, $generated.Replace($script:AutoEraseScripts['dnsCache'], 'Write-Output "no cleanup"'))
    $noCleanup = Get-AutoEraseSchedules
    if ($noCleanup.error -or $noCleanup.schedules[0].payloadVerified) { throw 'A valid script missing its cleanup body must not be verified.' }
    [IO.File]::WriteAllText($generatedPath, $generated + "`nif (`$true) {`n")
    $invalid = Get-AutoEraseSchedules
    if ($invalid.error -or $invalid.schedules[0].payloadVerified) { throw 'A script with a syntax error must not be verified.' }
    $interactiveArgs = ConvertTo-AutoEraseTaskArgument -CategoryId 'wslData' -IntervalMinutes 60 -Script $script:AutoEraseScripts['wslData'] -ExecutionScope 'user-fixture' -FirstDueUtc ([DateTime]::UtcNow.AddHours(1).ToString('o')) -AllowDefaultExcluded $true
    $interactiveTask = [pscustomobject]@{ Principal = [pscustomobject]@{ LogonType = 'Interactive'; RunLevel = 'Highest' }; Actions = @([pscustomobject]@{ Execute = 'powershell.exe'; Arguments = $interactiveArgs }) }
    $interactiveIdentity = @{ categoryId = 'wslData'; ownerSid = $sid }
    if (-not (Test-AutoEraseRunnablePayload -Task $interactiveTask -Identity $interactiveIdentity -IntervalMinutes 60)) { throw 'An interactive WSL task must be verifiable.' }
    if ($interactiveArgs -notmatch '-File\s+"([^"]+)"') { throw 'Protected fixture payload path missing.' }
    $protectedPath = $Matches[1]
    $protectedBody = [IO.File]::ReadAllText($protectedPath)
    [IO.File]::WriteAllText($protectedPath, $protectedBody.Replace('$allowDefaultExcluded = $true', '$allowDefaultExcluded = $false'))
    if (Test-AutoEraseRunnablePayload -Task $interactiveTask -Identity $interactiveIdentity -IntervalMinutes 60) { throw 'A protected task that refuses manual authorization must not be verified.' }
    [IO.File]::WriteAllText($protectedPath, $protectedBody)
    $interactiveTask.Principal.LogonType = 'S4U'
    if (Test-AutoEraseRunnablePayload -Task $interactiveTask -Identity $interactiveIdentity -IntervalMinutes 60) { throw 'A WSL task without an interactive session must not be verified.' }
} finally {
    $env:ProgramData = $oldProgramData
    Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
