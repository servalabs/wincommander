# ============================================================================
# PRODUCTIVITY MODULE
# Wrapper for Performance and Focus Tracking Engine
# ============================================================================

function Get-ProductivityProcesses {
    $session = (Get-Process -Id $PID -ErrorAction Stop).SessionId
    if ($null -eq $session) { throw 'Windows session identity is unavailable.' }
    $processes = @(Get-Process -Name 'aw-server','aw-server-rust','aw-tauri','aw-qt','aw-watcher-*' -ErrorAction SilentlyContinue)
    if (@($processes | Where-Object { $null -eq $_.SessionId }).Count) { throw 'ActivityWatch process session identity unavailable.' }
    @($processes | Where-Object { $_.SessionId -eq $session })
}

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
        
        $processes = @(Get-ProductivityProcesses)
        $running = @{
            server = $false
            input  = $false
            active = $false
            managed = $false
        }

        foreach ($p in $processes) {
            if ($p.Name -in @('aw-server','aw-server-rust','aw-tauri')) { $running.server = $true }
            if ($p.Name -in @('aw-tauri','aw-qt')) { $running.managed = $true }
            if ($p.Name -eq "aw-watcher-afk") { $running.input = $true }
            if ($p.Name -eq "aw-watcher-window") { $running.active = $true }
        }
        $running.apiReady = $false
        try {
            $info = Invoke-RestMethod -Uri 'http://127.0.0.1:5600/api/0/info' -Method Get -TimeoutSec 2 -ErrorAction Stop
            $running.apiReady = $null -ne $info.version
        } catch { }

        $duplicate = @($processes | Where-Object { $_.Name -in @('aw-server','aw-server-rust','aw-tauri') }).Count -gt 1 -or
                @($processes | Where-Object { $_.Name -eq 'aw-watcher-afk' }).Count -gt 1 -or
                @($processes | Where-Object { $_.Name -eq 'aw-watcher-window' }).Count -gt 1
        return @{
            duplicate = $duplicate
            installed = $installed
            running   = ($running.server -and $running.input -and $running.active -and $running.apiReady -and -not $duplicate)
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
        if ($status.error) { return $status }
        if ($status.duplicate) { throw 'Duplicate ActivityWatch components are running. No new tracker was started.' }
        if ($status.running) {
            return @{ success = $true; message = "Tracking internal processes already active." }
        }
        if (-not $status.details.server -and $status.details.apiReady) { throw 'Another Windows session owns the local ActivityWatch server.' }

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
            if ($binaries.managed -and ($status.details.input -or $status.details.active)) {
                throw 'ActivityWatch watchers are running without their manager. No new tracker was started.'
            }
            Start-Process -FilePath $binaries.server -WorkingDirectory (Split-Path $binaries.server) -WindowStyle Hidden -ErrorAction Stop | Out-Null
        }

        # Poll until server is ready
        $baseUri = "http://127.0.0.1:5600/api/0"
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
        $status = Get-ProductivityStatus
        if ($status.error) { return $status }
        if ($status.duplicate) { throw 'Duplicate ActivityWatch components are running.' }
        if (-not $status.details.server -or -not $status.details.apiReady) { throw 'ActivityWatch local server ownership could not be confirmed.' }
        if (-not $status.details.managed -and -not $status.details.input) {
            Start-Process -FilePath $binaries.afk -WorkingDirectory (Split-Path $binaries.afk) -WindowStyle Hidden -ErrorAction Stop | Out-Null
        }
        if (-not $status.details.managed -and -not $status.details.active) {
            Start-Process -FilePath $binaries.window -WorkingDirectory (Split-Path $binaries.window) -WindowStyle Hidden -ErrorAction Stop | Out-Null
        }

        for ($i = 0; $i -lt 20; $i++) {
            $status = Get-ProductivityStatus
            if ($status.running -and -not $status.duplicate) { return @{ success = $true; message = 'ActivityWatch is running.' } }
            if ($status.error -or $status.duplicate) { break }
            Start-Sleep -Milliseconds 250
        }
        throw 'ActivityWatch required watchers did not become healthy. Check its watcher settings.'
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
        managed = $false
    }

    $possibleDirs = @(
        "$env:LOCALAPPDATA\Programs\ActivityWatch",
        "$env:LOCALAPPDATA\Programs\ActivityWatch-Tauri",
        "$env:ProgramFiles\ActivityWatch",
        "$env:ProgramFiles\ActivityWatch-Tauri",
        "${env:ProgramFiles(x86)}\ActivityWatch",
        "${env:ProgramFiles(x86)}\ActivityWatch-Tauri"
    )

    # Add WinGet path if exists
    if (Test-Path "$env:LOCALAPPDATA\Microsoft\WinGet\Packages") {
        $awDirs = Get-ChildItem -Path "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Directory -ErrorAction SilentlyContinue | Where-Object { $_.Name -like "ActivityWatch*" }
        foreach ($d in $awDirs) {
            $possibleDirs += (Join-Path $d.FullName 'activitywatch')
            $possibleDirs += $d.FullName
        }
    }

    foreach ($dir in $possibleDirs) {
        if (-not (Test-Path $dir)) { continue }
        $results = @{ server = $null; afk = $null; window = $null; managed = $false }

        # Possible relative paths within the base dir
        $serverSubPaths = @('aw-tauri.exe', "aw-server\aw-server.exe", "aw-server-rust\aw-server-rust.exe", "aw-server-rust\aw-server.exe", "aw-server.exe")
        $afkSubPaths = @("aw-watcher-afk\aw-watcher-afk.exe", "aw-watcher-afk.exe")
        $windowSubPaths = @("aw-watcher-window\aw-watcher-window.exe", "aw-watcher-window.exe")

        foreach ($p in $serverSubPaths) {
            $full = Join-Path $dir $p
            if (Test-Path $full) { $results.server = $full; $results.managed = $p -eq 'aw-tauri.exe'; break }
        }
        foreach ($p in $afkSubPaths) {
            $full = Join-Path $dir $p
            if (Test-Path $full) { $results.afk = $full; break }
        }
        foreach ($p in $windowSubPaths) {
            $full = Join-Path $dir $p
            if (Test-Path $full) { $results.window = $full; break }
        }

        if ($results.server -and $results.afk -and $results.window) { return $results }
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
        foreach ($process in @(Get-ProductivityProcesses | Where-Object { $_.Name -in @('aw-tauri','aw-qt') })) {
            Stop-Process -Id $process.Id -Force -ErrorAction Stop
        }
        Start-Sleep -Milliseconds 250
        if (@(Get-ProductivityProcesses | Where-Object { $_.Name -in @('aw-tauri','aw-qt') }).Count -gt 0) {
            throw 'ActivityWatch manager is still running; watcher shutdown was not attempted.'
        }
        foreach ($process in @(Get-ProductivityProcesses)) { Stop-Process -Id $process.Id -Force -ErrorAction Stop }
        # Brief wait for port 5600 to be released before allowing restart
        Start-Sleep -Seconds 1.5
        if (@(Get-ProductivityProcesses).Count -gt 0) { throw 'ActivityWatch stop could not be confirmed.' }
        return @{ success = $true; message = "Tracking session terminated." }
    }
    catch {
        return @{ error = $true; message = $_.Exception.Message }
    }
}
