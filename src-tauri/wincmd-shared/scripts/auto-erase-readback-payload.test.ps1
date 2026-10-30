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
    $present = Get-AutoEraseSchedules
    if ($present.error -or -not $present.schedules[0].payloadVerified) { throw 'A readable matching payload should be verified.' }
    [IO.File]::WriteAllText($path, "`$categoryId = 'other'`n")
    $wrong = Get-AutoEraseSchedules
    if ($wrong.error -or $wrong.schedules[0].payloadVerified) { throw 'A mismatched payload must not be verified.' }
} finally {
    $env:ProgramData = $oldProgramData
    Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
