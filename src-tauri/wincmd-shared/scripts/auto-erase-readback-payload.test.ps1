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
        Principal = [pscustomobject]@{ UserId = $sid }
        Actions = @([pscustomobject]@{ Execute = 'powershell.exe'; Arguments = "-NoProfile -File `"$path`"" })
        Triggers = @([pscustomobject]@{ Repetition = [pscustomobject]@{ Interval = 'PT60M' } })
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
    $argument = ConvertTo-AutoEraseTaskArgument -CategoryId 'dnsCache' -IntervalMinutes 60 -Script $script:AutoEraseScripts['dnsCache'] -ExecutionScope 'user-fixture' -FirstDueUtc ([DateTime]::UtcNow.AddHours(1).ToString('o'))
    $script:fixtureTask.Actions[0].Arguments = $argument
    $present = Get-AutoEraseSchedules
    if ($present.error -or -not $present.schedules[0].payloadVerified) { throw 'A generated cleanup payload should be verified.' }
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
} finally {
    $env:ProgramData = $oldProgramData
    Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
