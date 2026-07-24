# ============================================================================
# PRODUCTIVITY MODULE
# Wrapper for Performance and Focus Tracking Engine
# ============================================================================

function Get-ProductivityStatus {
    try {
        $binaries = Get-ProductivityBinaries
        $installed = -not [string]::IsNullOrEmpty($binaries.server)
        
        $processes = Get-Process -Name "aw-server", "aw-watcher-afk", "aw-watcher-window" -ErrorAction SilentlyContinue
        $running = @{
            server = $false
            input  = $false
            active = $false
        }

        foreach ($p in $processes) {
            if ($p.Name -eq "aw-server") { $running.server = $true }
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
        if (-not $binaries.server) {
            return @{ error = $true; message = "Tracking Engine binaries not found. Please install via 'Packages & Apps'." }
        }

        # Start server
        $serverInfo = New-Object System.Diagnostics.ProcessStartInfo
        $serverInfo.FileName = $binaries.server
        $serverInfo.WorkingDirectory = Split-Path $binaries.server
        $serverInfo.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden
        $serverInfo.CreateNoWindow = $true
        [System.Diagnostics.Process]::Start($serverInfo) | Out-Null

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

        # Start watchers
        if ($binaries.afk -and (Test-Path $binaries.afk)) {
            Start-Process -FilePath $binaries.afk -WorkingDirectory (Split-Path $binaries.afk) -WindowStyle Hidden -CreateNoWindow
        }
        if ($binaries.window -and (Test-Path $binaries.window)) {
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
        $serverSubPaths = @("aw-server-rust\aw-server.exe", "aw-server\aw-server.exe", "aw-server.exe")
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

function Invoke-ProductivityEngineMaintenance {
    try {
        $stealthEnabled = Get-WCSetting -Path "productivity.productivityEngineStealthEnabled"
        if ($stealthEnabled -ne $true) {
            return @{ success = $true; message = "Productivity Engine stealth mode is not enabled." }
        }

        # Ensure aw-qt is NOT running
        $qt = Get-Process -Name "aw-qt" -ErrorAction SilentlyContinue
        if ($qt) {
            Stop-Process -Name "aw-qt" -Force -ErrorAction SilentlyContinue
        }

        # Check and start backends if missing
        $awServer = $null
        $possiblePaths = @(
            "$env:LOCALAPPDATA\Programs\ActivityWatch\aw-server.exe",
            "$env:LOCALAPPDATA\Programs\ActivityWatch\aw-server\aw-server.exe",
            "$env:ProgramFiles\ActivityWatch\aw-server.exe",
            "${env:ProgramFiles(x86)}\ActivityWatch\aw-server.exe"
        )
        foreach ($path in $possiblePaths) {
            if (Test-Path $path) { $awServer = $path; break }
        }

        if (-not $awServer) {
            return @{ error = $true; message = "Productivity Engine binaries not found." }
        }

        $binDir = Split-Path $awServer
        $components = @(
            "aw-server-rust\aw-server.exe",
            "aw-server\aw-server.exe",
            "aw-server.exe",
            "aw-watcher-afk\aw-watcher-afk.exe",
            "aw-watcher-window\aw-watcher-window.exe"
        )

        foreach ($comp in $components) {
            $compPath = Join-Path $binDir $comp
            if (-not (Test-Path $compPath)) {
                # Try relative to parent if binDir is a subfolder
                $compPath = Join-Path (Split-Path $binDir) $comp
            }
            
            if (Test-Path $compPath) {
                $procName = [System.IO.Path]::GetFileNameWithoutExtension($compPath)
                if (-not (Get-Process -Name $procName -ErrorAction SilentlyContinue)) {
                    Start-Process -FilePath $compPath -WorkingDirectory (Split-Path $compPath) -WindowStyle Hidden -CreateNoWindow -ErrorAction SilentlyContinue
                }
            }
        }

        return @{ success = $true; message = "Productivity Engine maintenance complete." }
    }
    catch {
        return @{ error = $true; message = $_.Exception.Message }
    }
}
