# ============================================================================
# PRODUCTIVITY MODULE
# Wrapper for Performance and Focus Tracking Engine
# ============================================================================

function Get-ProductivityStatus {
    try {
        $binaries = Get-ProductivityBinaries
        # Treat ActivityWatch as installed only when the server and both
        # first-party watchers are present.  A server alone is detectable but
        # cannot record useful activity, so the auto-start path must not claim
        # it is ready.
        $installed = -not [string]::IsNullOrEmpty($binaries.server) -and
            -not [string]::IsNullOrEmpty($binaries.afk) -and
            -not [string]::IsNullOrEmpty($binaries.window)
        
        $processes = Get-Process -Name "aw-server", "aw-server-rust", "aw-watcher-afk", "aw-watcher-window" -ErrorAction SilentlyContinue
        $running = @{
            server = $false
            input  = $false
            active = $false
        }

        foreach ($p in $processes) {
            if ($p.Name -eq "aw-server" -or $p.Name -eq "aw-server-rust") { $running.server = $true }
            if ($p.Name -eq "aw-watcher-afk") { $running.input = $true }
            if ($p.Name -eq "aw-watcher-window") { $running.active = $true }
        }

        return @{
            installed = $installed
            running   = ($running.server -and $running.input -and $running.active)
            details   = $running
        }
    }
    catch {
        return @{ error = $true; message = $_.Exception.Message }
    }
}
function Start-ProductivityTracker {
    try {
        $status = Get-ProductivityStatus
        if ($status.running) {
            return @{ success = $true; message = "Tracking internal processes already active." }
        }

        $binaries = Get-ProductivityBinaries
        $missing = @()
        if (-not $binaries.server) { $missing += "server" }
        if (-not $binaries.afk) { $missing += "AFK watcher" }
        if (-not $binaries.window) { $missing += "window watcher" }
        if ($missing.Count -gt 0) {
            return @{ error = $true; message = "ActivityWatch is incomplete (missing $($missing -join ', ')). Reinstall it from Packages & Apps." }
        }

        # Start only the components that are absent.  Starting aw-server a
        # second time when it already owns port 5600 produces a false-success
        # result and leaves the two actual watchers idle.
        if (-not $status.details.server) {
            $serverInfo = New-Object System.Diagnostics.ProcessStartInfo
            $serverInfo.FileName = $binaries.server
            $serverInfo.WorkingDirectory = Split-Path $binaries.server
            $serverInfo.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden
            $serverInfo.CreateNoWindow = $true
            [System.Diagnostics.Process]::Start($serverInfo) | Out-Null
        }

        # Poll until server is ready
        $baseUri = "http://localhost:5600/api/0"
        $ready = $false
        for ($i = 0; $i -lt 20; $i++) {
            Start-Sleep -Milliseconds 500
            try {
                $null = Invoke-RestMethod -Uri "$baseUri/info" -Method Get -TimeoutSec 2 -ErrorAction Stop
                $ready = $true
                break
            }
            catch { }
        }
        if (-not $ready) {
            return @{ error = $true; message = "Server failed to start. Port 5600 may still be in use. Try again in a few seconds." }
        }

        # Start whichever watcher is missing; preserve any existing watcher
        # state rather than creating duplicate instances.
        if (-not $status.details.input) {
            Start-Process -FilePath $binaries.afk -WorkingDirectory (Split-Path $binaries.afk) -WindowStyle Hidden -CreateNoWindow
        }
        if (-not $status.details.active) {
            Start-Process -FilePath $binaries.window -WorkingDirectory (Split-Path $binaries.window) -WindowStyle Hidden -CreateNoWindow
        }

        return @{ success = $true; message = "Tracking engine successfully initialized." }
    }
    catch {
        return @{ error = $true; message = $_.Exception.Message }
    }
}

function Get-ProductivityBinaries {
    $results = @{
        server = $null
        afk    = $null
        window = $null
    }

    $possibleDirs = @(
        "$env:LOCALAPPDATA\Programs\ActivityWatch",
        "$env:ProgramFiles\ActivityWatch",
        "${env:ProgramFiles(x86)}\ActivityWatch"
    )

    # Add WinGet path if exists
    if (Test-Path "$env:LOCALAPPDATA\Microsoft\WinGet\Packages") {
        $awDirs = Get-ChildItem -Path "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Directory -ErrorAction SilentlyContinue | Where-Object { $_.Name -like "ActivityWatch*" }
        foreach ($d in $awDirs) {
            $possibleDirs += $d.FullName
        }
    }

    foreach ($dir in $possibleDirs) {
        if (-not (Test-Path $dir)) { continue }

        # Possible relative paths within the base dir
        $serverSubPaths = @("aw-server\aw-server.exe", "aw-server-rust\aw-server-rust.exe", "aw-server-rust\aw-server.exe", "aw-server.exe")
        $afkSubPaths = @("aw-watcher-afk\aw-watcher-afk.exe", "aw-watcher-afk.exe")
        $windowSubPaths = @("aw-watcher-window\aw-watcher-window.exe", "aw-watcher-window.exe")

        foreach ($p in $serverSubPaths) {
            $full = Join-Path $dir $p
            if (Test-Path $full) { $results.server = $full; break }
        }
        foreach ($p in $afkSubPaths) {
            $full = Join-Path $dir $p
            if (Test-Path $full) { $results.afk = $full; break }
        }
        foreach ($p in $windowSubPaths) {
            $full = Join-Path $dir $p
            if (Test-Path $full) { $results.window = $full; break }
        }

        if ($results.server) { break }
    }

    # Fallback to Get-Command
    if (-not $results.server) {
        $cmd = Get-Command "aw-server" -ErrorAction SilentlyContinue
        if ($cmd) { $results.server = $cmd.Source }
    }

    return $results
}

function Stop-ProductivityTracker {
    try {
        Stop-Process -Name "aw-watcher-window", "aw-watcher-afk", "aw-server" -Force -ErrorAction SilentlyContinue
        # Brief wait for port 5600 to be released before allowing restart
        Start-Sleep -Seconds 1.5
        return @{ success = $true; message = "Tracking session terminated." }
    }
    catch {
        return @{ error = $true; message = $_.Exception.Message }
    }
}
