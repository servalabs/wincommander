# Per-session half of the multi-user development switcher.
# It runs as the signed-in user through a temporary Scheduled Task.  That is
# essential: Windows prevents an administrator's desktop process from opening
# a GUI directly on another user's desktop.

[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$StatePath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Get-State {
    if (-not (Test-Path -LiteralPath $StatePath)) { return $null }
    try { return Get-Content -LiteralPath $StatePath -Raw | ConvertFrom-Json } catch { return $null }
}

function Get-CurrentSessionAppProcesses {
    $sessionId = (Get-Process -Id $PID).SessionId
    foreach ($name in @("wincommander-free", "wincommander-pro", "WinCommander")) {
        foreach ($process in @(Get-Process -Name $name -ErrorAction SilentlyContinue)) {
            if ($process.Id -eq $PID -or $process.SessionId -ne $sessionId) { continue }
            try {
                $path = [IO.Path]::GetFullPath($process.Path)
                [pscustomobject]@{ Process = $process; Path = $path }
            } catch {
                # A process whose executable path cannot be read is not ours to
                # stop.  Failing closed here avoids terminating unrelated work.
            }
        }
    }
}

function Stop-ProcessesAtPath([string]$ExecutablePath) {
    $target = [IO.Path]::GetFullPath($ExecutablePath)
    Get-CurrentSessionAppProcesses |
        Where-Object { [string]::Equals($_.Path, $target, [StringComparison]::OrdinalIgnoreCase) } |
        ForEach-Object { Stop-Process -Id $_.Process.Id -Force -ErrorAction SilentlyContinue }
}

function Stop-NormalSessionProcesses([string]$DebugExecutablePath) {
    $debugPath = [IO.Path]::GetFullPath($DebugExecutablePath)
    Get-CurrentSessionAppProcesses |
        Where-Object { -not [string]::Equals($_.Path, $debugPath, [StringComparison]::OrdinalIgnoreCase) } |
        ForEach-Object { Stop-Process -Id $_.Process.Id -Force -ErrorAction SilentlyContinue }
}

function Stop-DevelopmentSessionProcesses([string]$DebugExecutablePath) {
    $debugPath = [IO.Path]::GetFullPath($DebugExecutablePath)
    Get-CurrentSessionAppProcesses |
        Where-Object {
            [string]::Equals($_.Path, $debugPath, [StringComparison]::OrdinalIgnoreCase) -or
            $_.Process.ProcessName -eq "wincommander-pro"
        } |
        ForEach-Object { Stop-Process -Id $_.Process.Id -Force -ErrorAction SilentlyContinue }
}

$state = Get-State
if ($null -eq $state -or -not $state.active) { exit 0 }

$devExecutable = [IO.Path]::GetFullPath([string]$state.devExecutable)
if (-not (Test-Path -LiteralPath $devExecutable -PathType Leaf)) {
    throw "The debug executable recorded for this development run does not exist: $devExecutable"
}

$sessionRoot = Join-Path $env:LOCALAPPDATA "WinCommander\\dev-multiuser"
New-Item -ItemType Directory -Path $sessionRoot -Force | Out-Null
$sessionFile = Join-Path $sessionRoot ("{0}.json" -f $state.runId)

# Record only the installed executable(s) actually running in this user's
# session.  On stop we restore exactly these, never launch WinCommander for a
# person who did not have it open before the test.
$releaseExecutables = @(Get-CurrentSessionAppProcesses |
    Where-Object {
        $_.Process.ProcessName -ne "wincommander-pro" -and
        -not [string]::Equals($_.Path, $devExecutable, [StringComparison]::OrdinalIgnoreCase)
    } |
    ForEach-Object { $_.Path } |
    Select-Object -Unique)

@{
    releaseExecutables = $releaseExecutables
    runId = [string]$state.runId
} | ConvertTo-Json | Set-Content -LiteralPath $sessionFile -Encoding UTF8

Stop-NormalSessionProcesses $devExecutable

if (-not @(Get-CurrentSessionAppProcesses |
        Where-Object { [string]::Equals($_.Path, $devExecutable, [StringComparison]::OrdinalIgnoreCase) })) {
    Start-Process -FilePath $devExecutable -WorkingDirectory (Split-Path -Parent $devExecutable)
}

while ($true) {
    Start-Sleep -Seconds 2
    $state = Get-State
    if ($null -eq $state -or -not $state.active -or [string]$state.runId -ne [IO.Path]::GetFileNameWithoutExtension($sessionFile)) {
        break
    }
}

Stop-DevelopmentSessionProcesses $devExecutable

if (Test-Path -LiteralPath $sessionFile) {
    $restore = Get-Content -LiteralPath $sessionFile -Raw | ConvertFrom-Json
    foreach ($releaseExecutable in @($restore.releaseExecutables)) {
        if (-not (Test-Path -LiteralPath $releaseExecutable -PathType Leaf)) { continue }
        try {
            Start-Process -FilePath $releaseExecutable -WorkingDirectory (Split-Path -Parent $releaseExecutable)
        } catch {
            # A production build requires elevation.  This can only restore
            # automatically for a session whose account is allowed to launch it.
            Write-Warning ("Could not restore {0}: {1}" -f $releaseExecutable, $_.Exception.Message)
        }
    }
}

Remove-Item -LiteralPath $sessionFile -Force -ErrorAction SilentlyContinue
