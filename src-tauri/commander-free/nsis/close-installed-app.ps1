[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateNotNullOrEmpty()]
    [string]$ExecutablePath,

    [ValidateRange(0, 30)]
    [int]$GraceSeconds = 5,

    [ValidateRange(0, 30)]
    [int]$ForceWaitSeconds = 5
)

# The installer runs elevated, but it must not terminate every process sharing
# an executable name.  Resolve the requested Program Files binary and match
# Win32_Process.ExecutablePath exactly before asking Windows to close it.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

try {
    $targetPath = [System.IO.Path]::GetFullPath($ExecutablePath)
}
catch {
    Write-Error "The WinCommander executable path is invalid: $ExecutablePath"
    exit 1
}

if (-not (Test-Path -LiteralPath $targetPath -PathType Leaf)) {
    Write-Output "No installed WinCommander executable exists at $targetPath."
    exit 0
}

function Get-InstalledWinCommanderProcessId {
    $matchingIds = [System.Collections.Generic.List[int]]::new()
    $processes = Get-CimInstance -ClassName Win32_Process -Filter "Name='wincommander-free.exe'"

    foreach ($process in $processes) {
        if ([string]::IsNullOrWhiteSpace($process.ExecutablePath)) {
            continue
        }

        try {
            $processPath = [System.IO.Path]::GetFullPath($process.ExecutablePath)
        }
        catch {
            continue
        }

        if ([string]::Equals(
                $processPath,
                $targetPath,
                [System.StringComparison]::OrdinalIgnoreCase
            )) {
            $matchingIds.Add([int]$process.ProcessId)
        }
    }

    return @($matchingIds | Sort-Object -Unique)
}

function Wait-ForInstalledWinCommanderExit {
    param([int]$TimeoutSeconds)

    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    do {
        $remaining = @(Get-InstalledWinCommanderProcessId)
        if ($remaining.Count -eq 0) {
            return @()
        }

        if ($stopwatch.Elapsed.TotalSeconds -ge $TimeoutSeconds) {
            return $remaining
        }

        Start-Sleep -Milliseconds 250
    } while ($true)
}

try {
    $initial = @(Get-InstalledWinCommanderProcessId)
}
catch {
    Write-Error "Could not inspect the installed WinCommander process: $($_.Exception.Message)"
    exit 1
}

if ($initial.Count -eq 0) {
    Write-Output "The installed WinCommander application is not running."
    exit 0
}

Write-Output "Requesting a normal close for installed WinCommander process IDs: $($initial -join ', ')."
foreach ($processId in $initial) {
    # Without /F taskkill sends a normal close request to a GUI process. The
    # later forced pass is used only when that bounded grace period expires.
    # PowerShell 7 can turn a non-zero native exit code into a terminating
    # NativeCommandError when ErrorActionPreference is Stop.  A normal close
    # refusal is precisely the case that must continue to the bounded /F
    # fallback, so retain the diagnostic but never let this first attempt
    # abort the updater.
    try {
        & "$env:SystemRoot\System32\taskkill.exe" /PID $processId 2>$null | Write-Output
        if ($LASTEXITCODE -ne 0) {
            Write-Output "Normal close request for WinCommander process ID $processId returned exit code $LASTEXITCODE; waiting before forced shutdown."
        }
    }
    catch {
        Write-Output "Normal close request for WinCommander process ID $processId was refused; waiting before forced shutdown: $($_.Exception.Message)"
    }
}

try {
    $remaining = @(Wait-ForInstalledWinCommanderExit -TimeoutSeconds $GraceSeconds)
}
catch {
    Write-Error "Could not confirm that WinCommander exited: $($_.Exception.Message)"
    exit 1
}

if ($remaining.Count -gt 0) {
    Write-Output "Forcing only the installed WinCommander process IDs: $($remaining -join ', ')."
    foreach ($processId in $remaining) {
        # As above, retain taskkill diagnostics but let the subsequent exact
        # process re-check decide success.  This makes an access-denied /F
        # attempt a clear controlled installer failure rather than a hidden
        # PowerShell pipeline failure.
        try {
            & "$env:SystemRoot\System32\taskkill.exe" /PID $processId /F 2>$null | Write-Output
            if ($LASTEXITCODE -ne 0) {
                Write-Output "Forced close request for WinCommander process ID $processId returned exit code $LASTEXITCODE."
            }
        }
        catch {
            Write-Output "Forced close request for WinCommander process ID $processId failed: $($_.Exception.Message)"
        }
    }

    try {
        $remaining = @(Wait-ForInstalledWinCommanderExit -TimeoutSeconds $ForceWaitSeconds)
    }
    catch {
        Write-Error "Could not confirm that forced WinCommander shutdown completed: $($_.Exception.Message)"
        exit 1
    }
}

if ($remaining.Count -gt 0) {
    Write-Error "WinCommander process IDs still lock the installed executable: $($remaining -join ', ')."
    exit 1
}

Write-Output "The installed WinCommander executable is no longer locked by WinCommander."
