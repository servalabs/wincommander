# ============================================================================
# DEPENDENCIES MODULE
# Centralized dependency checking, installation, hiding, and starting.
# ============================================================================
#
# DESIGN:
#   Each dependency has 4 per-app functions:
#     Test-<Dep>Installed   — fast boolean check
#     Install-<Dep>         — installs via winget or custom logic
#     Hide-<Dep>            — removes shortcuts, registry entries, tray icons
#     Start-<Dep>           — ensures the service/process is running
#
#   Orchestrator functions combine these per-app primitives:
#     Get-DependencyStatus       — checks ALL deps, returns status array
#     Install-Dependency         — install a single dep by ID
#     Install-AllDependencies    — install + hide + start everything missing
#     Hide-AllBackendApps        — hide all deps (replaces old Hide-BackendApps)
#
# FLOW (per-dep):
#   Test installed → no → Install → Hide → Start
#   Test installed → yes, not running → Start
#   Test installed → yes, running → OK
# ============================================================================


# ────────────────────────────────────────────────────────────────────────────
# PERSISTENT CACHE — 12-hour file-backed dependency status
# ────────────────────────────────────────────────────────────────────────────
$script:DEP_CACHE_TTL_SECONDS = 43200   # 12 hours
$script:DEP_CACHE_ROOT        = if ($env:ProgramData) { Join-Path $env:ProgramData "WinCommander" } elseif ($env:LOCALAPPDATA) { Join-Path $env:LOCALAPPDATA "WinCommander" } else { Join-Path $env:TEMP "WinCommander" }
New-Item -ItemType Directory -Force -Path $script:DEP_CACHE_ROOT -ErrorAction SilentlyContinue | Out-Null
$script:DEP_CACHE_FILE        = Join-Path $script:DEP_CACHE_ROOT "dep_status_cache.json"

function Read-DepStatusCache {
    # Returns @{ status = [...]; cacheAgeSecs = N } if cache is fresh, $null otherwise.
    if (-not (Test-Path $script:DEP_CACHE_FILE)) { return $null }
    try {
        $raw = Get-Content $script:DEP_CACHE_FILE -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
        if (-not $raw.savedAt -or -not $raw.status) { return $null }
        $ageSecs = [int]((Get-Date) - [datetime]$raw.savedAt).TotalSeconds
        if ($ageSecs -gt $script:DEP_CACHE_TTL_SECONDS) { return $null }
        return @{ status = $raw.status; cacheAgeSecs = $ageSecs }
    } catch { return $null }   # corrupt / unreadable — fall through to live probe
}

function Write-DepStatusCache($statusArray) {
    try {
        $dir = Split-Path $script:DEP_CACHE_FILE
        if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
        [pscustomobject]@{ savedAt = (Get-Date -Format 'o'); status = $statusArray } |
            ConvertTo-Json -Depth 10 |
            Set-Content $script:DEP_CACHE_FILE -Encoding UTF8 -ErrorAction Stop
    } catch { <# fail silently — cache is best-effort #> }
}

function Update-DepStatusCacheEntry([string]$depId, [hashtable]$mergeProps) {
    # Bust only one dep's entry in the file cache after a successful install.
    try {
        $raw = Get-Content $script:DEP_CACHE_FILE -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
        if (-not $raw.status) { return }
        foreach ($entry in $raw.status) {
            if ($entry.id -eq $depId) {
                foreach ($key in $mergeProps.Keys) { $entry.$key = $mergeProps[$key] }
            }
        }
        $raw | ConvertTo-Json -Depth 10 | Set-Content $script:DEP_CACHE_FILE -Encoding UTF8 -ErrorAction Stop
    } catch { <# fail silently #> }
}

# ────────────────────────────────────────────────────────────────────────────
# REGISTRY: dependency definitions
# ────────────────────────────────────────────────────────────────────────────

function Get-DependencyRegistry {
    return @(
        @{
            id       = 'encryptionEngine'
            name     = 'Encryption Engine'
            wingetId = 'IDRIX.VeraCrypt'
            panelId  = 'vault'
            canStart = $false   # VeraCrypt is on-demand, no background service
            canHide  = $true
        },
        @{
            # RAM disk engine. RAM Disk Engine isn't on winget so wingetId stays
            # $null; Install-RamDiskEngine does a direct SourceForge download
            # with silent /S install. Same UX as the other deps from the
            # user's POV — one click, no extra prompts.
            id       = 'ramDiskEngine'
            name     = 'RAM Disk Engine'
            wingetId = $null
            panelId  = 'vault'
            canStart = $false
            canHide  = $true
        },
        @{
            id       = 'meshVpn'
            name     = 'Private Mesh VPN'
            wingetId = 'Tailscale.Tailscale'
            panelId  = 'private-mesh'
            canStart = $true
            canHide  = $true
        },
        @{
            id       = 'productivityEngine'
            name     = 'Productivity Engine'
            wingetId = 'ActivityWatch.ActivityWatch'
            panelId  = 'productivity'
            canStart = $true
            canHide  = $true
        },
        @{
            id       = 'winget'
            name     = 'Package Manager'
            wingetId = $null    # Special case — winget installs itself
            panelId  = 'apps'
            canStart = $false
            canHide  = $false
        },
        @{
            id       = 'chocolatey'
            name     = 'Chocolatey'
            wingetId = $null    # Installed via its own bootstrap script, not winget
            panelId  = 'apps'
            canStart = $false
            canHide  = $false
        },
        @{
            id       = 'scoop'
            name     = 'Scoop'
            wingetId = $null    # Installed via its own bootstrap script, not winget
            panelId  = 'apps'
            canStart = $false
            canHide  = $false
        },
        @{
            id       = 'powershell7'
            name     = 'PowerShell 7'
            wingetId = 'Microsoft.PowerShell'
            panelId  = $null
            canStart = $false
            canHide  = $false
        },
        @{
            id       = 'vcredist'
            name     = 'Visual C++ Redistributable'
            wingetId = $null    # Multiple packages installed together
            panelId  = $null
            canStart = $false
            canHide  = $false
        },
        @{
            id       = 'privacyShieldAI'
            name     = 'Privacy AI Shield'
            wingetId = $null    # Python + pip packages, not a single winget app
            panelId  = 'privacy'
            canStart = $false   # Started on-demand from privacy panel
            canHide  = $false
        },
        @{
            id       = 'systemCleaner'
            name     = 'System Cleaner'
            wingetId = 'BleachBit.BleachBit'
            panelId  = $null
            canStart = $false   # On-demand via lockdown
            canHide  = $true
        },
        @{
            id       = 'instantSearch'
            name     = 'Instant Search Engine'
            wingetId = 'voidtools.Everything'
            panelId  = 'search-files'
            canStart = $false   # The search daemon (Everything.exe) is a separate app; CLI is on-demand
            canHide  = $true
        },
        @{
            id       = 'diskHealthEngine'
            name     = 'Disk Health Engine'
            wingetId = 'smartmontools.smartmontools'
            panelId  = 'dashboard'
            canStart = $false
            canHide  = $false
        },
        @{
            # Optional: enables HEIC / MP4 / MOV / TIFF / DNG / WebP / SVG
            # / RAW / RIFF in the metadata scrubber. Without it the
            # scrubber still works for JPEG / PNG / PDF / Office via our
            # pure-Rust handlers; with it, the dialog opens up every
            # format ExifTool understands (~100).
            id       = 'metadataScrubber'
            name     = 'Hidden Data Remover'
            wingetId = 'OliverBetz.ExifTool'
            panelId  = $null   # surfaced from the sidebar "Scrub Meta" action
            canStart = $false  # CLI-only, invoked on-demand
            canHide  = $false
        },
        @{
            # Local AI Advisor engine (Ollama). The FREE dependency is the
            # engine only; pulling a model is a long, progress-bearing
            # operation handled by the PAID Pull-OllamaModel Pro handler in
            # the AI Advisor panel (spec 13 / #10). "installed" = ollama.exe
            # present; "running" = the local server answers on 11434.
            id       = 'localLlm'
            name     = 'Local AI Advisor'      # whitelabel-neutral display name
            wingetId = 'Ollama.Ollama'
            panelId  = 'advisor'
            canStart = $true                   # ollama runs a background server we ensure is up
            canHide  = $false                  # headless server, no shortcuts/tray to scrub
        }
    )
}


# ════════════════════════════════════════════════════════════════════════════
# PER-DEPENDENCY: TEST INSTALLED
# ════════════════════════════════════════════════════════════════════════════

function Test-EncryptionEngineInstalled {
    $vc64 = "$env:ProgramFiles\VeraCrypt\VeraCrypt.exe"
    $vc86 = "${env:ProgramFiles(x86)}\VeraCrypt\VeraCrypt.exe"
    $veraPath = if (Test-Path $vc64 -ErrorAction SilentlyContinue) { $vc64 }
                elseif (Test-Path $vc86 -ErrorAction SilentlyContinue) { $vc86 }
                else { $null }
    $installed = $null -ne $veraPath
    $version = $null
    if ($installed) {
        $version = try { (Get-Item $veraPath).VersionInfo.ProductVersion } catch { $null }
    }
    return @{ installed = $installed; version = $version }
}

function Test-MeshVpnInstalled {
    $installed = $false
    $version = $null
    $connected = $null

    if ((Get-Command tailscale.exe -ErrorAction SilentlyContinue) -or
        (Test-Path "$env:ProgramFiles\Tailscale\tailscale.exe") -or
        (Test-Path "${env:ProgramFiles(x86)}\Tailscale\tailscale.exe")) {
        $installed = $true
        $meshExePath = if (Get-Command tailscale.exe -EA SilentlyContinue) {
            (Get-Command tailscale.exe).Source
        }
        elseif (Test-Path "$env:ProgramFiles\Tailscale\tailscale.exe") {
            "$env:ProgramFiles\Tailscale\tailscale.exe"
        }
        else {
            "${env:ProgramFiles(x86)}\Tailscale\tailscale.exe"
        }
        $version = try { (Get-Item $meshExePath).VersionInfo.ProductVersion } catch { $null }

        # Only call `tailscale status` if the service is running — otherwise
        # the CLI blocks waiting for the daemon and adds 1-2 s to every check.
        try {
            $tsSvc = Get-Service -Name Tailscale -ErrorAction SilentlyContinue
            if ($tsSvc -and $tsSvc.Status -eq 'Running') {
                $meshExe = if (Get-Command tailscale.exe -EA SilentlyContinue) { "tailscale" } else { $meshExePath }
                $statusJson = & $meshExe status --json 2>$null | Out-String | ConvertFrom-Json
                $connected = $statusJson.BackendState -eq "Running"
            } else {
                $connected = $false
            }
        }
        catch { $connected = $false }
    }

    return @{ installed = $installed; version = $version; connected = $connected }
}

function Test-ProductivityEngineInstalled {
    $installed = $false
    $running = $false
    $version = $null

    # Primary: check if aw-server is already running (authoritative — no file needed)
    $running = $null -ne (Get-Process -Name "aw-server" -ErrorAction SilentlyContinue)
    if ($running) { $installed = $true }

    # Secondary: known install paths — file presence = installed
    $awPaths = @(
        "$env:LOCALAPPDATA\Programs\ActivityWatch\aw-server.exe",
        "$env:ProgramFiles\ActivityWatch\aw-server.exe",
        "${env:ProgramFiles(x86)}\ActivityWatch\aw-server.exe",
        "$env:LOCALAPPDATA\Programs\ActivityWatch\aw-server\aw-server.exe"
    )
    $awExePath = $null
    foreach ($p in $awPaths) {
        if (Test-Path $p -ErrorAction SilentlyContinue) { $installed = $true; $awExePath = $p; break }
    }

    if (-not $installed -and (Get-Command "aw-server" -ErrorAction SilentlyContinue)) {
        $installed = $true
        $awExePath = (Get-Command "aw-server").Source
    }

    # WinGet packages path fallback
    if (-not $installed -and (Test-Path "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -ErrorAction SilentlyContinue)) {
        $awDirs = Get-ChildItem -Path "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Directory -EA SilentlyContinue | Where-Object { $_.Name -like "ActivityWatch*" }
        foreach ($d in $awDirs) {
            $found = Get-ChildItem -Path $d.FullName -Filter "aw-server.exe" -Recurse -EA SilentlyContinue | Select-Object -First 1
            if ($found) { $installed = $true; $awExePath = $found.FullName; break }
        }
    }

    if ($awExePath) {
        $version = try { (Get-Item $awExePath).VersionInfo.ProductVersion } catch { $null }
    }

    return @{ installed = $installed; version = $version; running = $running }
}

function Get-LocalWingetPath {
    if (Get-Command "Resolve-WingetPath" -ErrorAction SilentlyContinue) {
        return Resolve-WingetPath
    }
    
    $wingetCmd = (Get-Command winget -ErrorAction SilentlyContinue).Source
    if ($wingetCmd) { return $wingetCmd }

    $candidates = @(
        "$env:LOCALAPPDATA\Microsoft\WindowsApps\winget.exe",
        "$env:LOCALAPPDATA\Microsoft\WinGet\Links\winget.exe"
    )
    foreach ($path in $candidates) {
        if (Test-Path $path) { return $path }
    }
    return $null
}

function Test-WingetDependencyInstalled {
    $wingetCmd = Get-LocalWingetPath

    $installed = $null -ne $wingetCmd
    $version = $null
    if ($installed) {
        # AppxPackage lookup is ~50 ms vs spawning winget --version (~500 ms).
        try {
            $pkg = Get-AppxPackage -Name 'Microsoft.DesktopAppInstaller' -ErrorAction SilentlyContinue |
                   Select-Object -First 1
            if ($pkg) { $version = $pkg.Version }
        }
        catch { }
    }

    return @{ installed = $installed; version = $version; missing = @() }
}

function Get-LocalChocolateyPath {
    $cmd = Get-Command choco.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }

    $root = if ($env:ChocolateyInstall) { $env:ChocolateyInstall } else { "$env:ProgramData\chocolatey" }
    $candidate = Join-Path $root "bin\choco.exe"
    if (Test-Path $candidate) { return $candidate }
    return $null
}

function Test-ChocolateyInstalled {
    $chocoCmd = Get-LocalChocolateyPath
    $installed = $null -ne $chocoCmd
    $version = $null
    if ($installed) {
        $version = try { (& $chocoCmd --version 2>$null | Select-Object -First 1) } catch { $null }
    }
    return @{ installed = $installed; version = $version }
}

function Get-LocalScoopPath {
    $cmd = Get-Command scoop.cmd -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }

    $roots = @()
    if ($env:SCOOP) { $roots += $env:SCOOP }
    $roots += "$env:USERPROFILE\scoop"
    $roots += "$env:ProgramData\scoop"
    foreach ($root in $roots) {
        $candidate = Join-Path $root "shims\scoop.cmd"
        if (Test-Path $candidate) { return $candidate }
    }
    return $null
}

function Test-ScoopInstalled {
    $scoopCmd = Get-LocalScoopPath
    $installed = $null -ne $scoopCmd
    $version = $null
    if ($installed) {
        $version = try { (& $scoopCmd --version 2>$null | Select-Object -First 1) } catch { $null }
    }
    return @{ installed = $installed; version = $version }
}

function Find-Python312Exe {
    # Refresh PATH to catch new installs
    $env:Path = [System.Environment]::GetEnvironmentVariable("Path", "Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path", "User")

    # Helper: validate an exe is real Python 3.12.x (not Store stub, not 3.13+)
    function Test-IsPython312 {
        param([string]$Exe)
        if (-not (Test-Path $Exe)) { return $false }
        if ($Exe -match "WindowsApps") { return $false }
        try {
            $out = & $Exe --version 2>&1 | Out-String
            if ($out -match 'Python (3\.12\.\d+)') { return $true }
        }
        catch {}
        return $false
    }

    # 1. Check well-known Python 3.12 install directories first. These paths
    # contain "Python312" verbatim — version is implicit, no need to spawn the
    # exe just to read its --version output. File existence is enough.
    $fixedCandidates = @(
        "$env:LOCALAPPDATA\Programs\Python\Python312\python.exe",
        "$env:ProgramFiles\Python312\python.exe",
        "$env:ProgramFiles(x86)\Python312\python.exe",
        "C:\Python312\python.exe"
    )
    foreach ($c in $fixedCandidates) {
        if (Test-Path $c -ErrorAction SilentlyContinue) { return $c }
    }

    # 2. Walk PATH entries looking for a python.exe that reports 3.12. Only
    # here do we need the subprocess — paths like C:\Python\ don't reveal
    # the version, so we have to ask the binary itself.
    foreach ($dir in ($env:Path -split ';')) {
        if ([string]::IsNullOrWhiteSpace($dir)) { continue }
        if ($dir -match 'WindowsApps') { continue }
        $candidate = Join-Path $dir 'python.exe'
        if (Test-IsPython312 $candidate) { return $candidate }
    }

    return $null
}

function Test-PrivacyShieldAIInstalled {
    # mediapipe only supports Python <= 3.12 — find a 3.12 interpreter specifically.
    # Cache the result in script scope so repeat Get-DependencyStatus calls in
    # the same session don't re-walk PATH + spawn `python --version` again.
    if (-not (Test-Path Variable:script:_cachedPython312Exe)) {
        $script:_cachedPython312Exe = Find-Python312Exe
    }
    $pythonExe = $script:_cachedPython312Exe

    if (-not $pythonExe) {
        # Name matches the "Python" entry in $allPackages so Get-AIDependenciesStatus
        # correctly flags it as missing. If we return "Python 3.12" here, the UI
        # silently treats Python as installed and the install flow never triggers.
        return @{ installed = $false; version = $null; missing = @("Python", "mediapipe", "opencv-python", "PyQt6", "numpy", "Pillow") }
    }

    # find_spec() instead of __import__(): locates packages by filesystem scan,
    # no C extensions loaded.  __import__ triggered full mediapipe + opencv init
    # (TensorFlow-lite etc.) which alone took 3-8 s.  find_spec does all 5 in ~100 ms.
    $checkSrc = @'
import sys, importlib.util
checks = [('mediapipe','mediapipe'),('cv2','opencv-python'),('PyQt6','PyQt6'),('numpy','numpy'),('PIL','Pillow')]
missing = [pkg for name, pkg in checks if importlib.util.find_spec(name) is None]
sys.stdout.write('|'.join(missing))
'@
    $output = & $pythonExe -c $checkSrc 2>$null
    $missing = if ([string]::IsNullOrWhiteSpace($output)) { @() } else { @($output -split '\|' | Where-Object { $_ }) }

    $allInstalled = ($missing.Count -eq 0)
    return @{ installed = $allInstalled; version = $null; missing = $missing }
}

function Get-AIDependenciesStatus {
    $testResult = Test-PrivacyShieldAIInstalled
    $allPackages = @("Python", "mediapipe", "opencv-python", "PyQt6", "numpy", "Pillow")
    $details = @()
    
    foreach ($pkg in $allPackages) {
        $isMissing = $testResult.missing -contains $pkg
        $details += @{
            name   = $pkg
            status = if ($isMissing) { "missing" } else { "installed" }
        }
    }

    return @{ 
        installed = $testResult.installed
        missing   = $testResult.missing
        details   = $details
    }
}

function Test-PowerShell7Installed {
    # Covers three install methods:
    #   1. Traditional MSI installer -> Program Files\PowerShell\7\pwsh.exe
    #   2. winget/user PATH install -> resolvable via Get-Command
    #   3. Microsoft Store (MSIX) install -> registers an App Execution Alias in
    #      %LOCALAPPDATA%\Microsoft\WindowsApps, which Windows does NOT resolve
    #      from an elevated process (this app runs requireAdministrator), so
    #      Get-Command/PATH lookups silently miss it even though it's installed.
    #      Get-AppxPackage queries the package repository directly and is
    #      unaffected by elevation or PATH.
    $installed = $false
    $version = $null
    $ps7Path = "$env:ProgramFiles\PowerShell\7\pwsh.exe"
    if (Test-Path $ps7Path) {
        $installed = $true
        $version = try { (Get-Item $ps7Path).VersionInfo.ProductVersion } catch { $null }
    }
    elseif (Get-Command pwsh -ErrorAction SilentlyContinue) {
        $pwsh = Get-Command pwsh
        if ($pwsh.Version.Major -ge 7) {
            $installed = $true
            $version = $pwsh.Version.ToString()
        }
    }
    else {
        $appx = try { Get-AppxPackage -Name "Microsoft.PowerShell" -ErrorAction Stop } catch { $null }
        if ($appx -and $appx.Version) {
            $installed = $true
            $version = $appx.Version
        }
    }
    return @{ installed = $installed; version = $version }
}

function Test-VCRedistInstalled {
    # Check for VCRedist 2015+ x64 (covers 2015, 2017, 2019, 2022)
    $installed = $false
    $version = $null
    $keys = @(
        'HKLM:\SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\X64',
        'HKLM:\SOFTWARE\WOW6432Node\Microsoft\VisualStudio\14.0\VC\Runtimes\X64'
    )
    foreach ($key in $keys) {
        if (Test-Path $key) {
            $installed = $true
            $version = try { (Get-ItemProperty $key -ErrorAction SilentlyContinue).Version } catch { $null }
            break
        }
    }
    return @{ installed = $installed; version = $version }
}

function Find-SystemCleanerExe {
    $paths = @(
        "${env:ProgramFiles(x86)}\BleachBit\bleachbit_console.exe",
        "$env:ProgramFiles\BleachBit\bleachbit_console.exe",
        "$env:LOCALAPPDATA\Microsoft\WinGet\Links\bleachbit_console.exe",
        "$env:ProgramFiles\WinGet\Links\bleachbit_console.exe",
        "$env:ProgramData\Microsoft\WinGet\Links\bleachbit_console.exe"
    )

    $cmd = Get-Command bleachbit_console.exe -ErrorAction SilentlyContinue
    if ($cmd -and $cmd.Source) { $paths += $cmd.Source }

    $uninstallRoots = @(
        'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
        'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall',
        'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall'
    )
    foreach ($root in $uninstallRoots) {
        if (-not (Test-Path $root)) { continue }
        Get-ChildItem $root -ErrorAction SilentlyContinue | ForEach-Object {
            try {
                $props = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
                if ("$($props.DisplayName)" -match 'BleachBit') {
                    foreach ($base in @($props.InstallLocation, (Split-Path ("$($props.DisplayIcon)" -replace ',\s*-?\d+\s*$', '') -Parent))) {
                        if (-not [string]::IsNullOrWhiteSpace("$base")) {
                            $paths += (Join-Path "$base" 'bleachbit_console.exe')
                        }
                    }
                }
            } catch {}
        }
    }

    foreach ($root in @(
        "$env:LOCALAPPDATA\Microsoft\WinGet\Packages",
        "$env:ProgramFiles\WinGet\Packages",
        "$env:ProgramData\Microsoft\WinGet\Packages"
    )) {
        if (Test-Path $root) {
            Get-ChildItem -Path $root -Filter 'bleachbit_console.exe' -Recurse -ErrorAction SilentlyContinue |
                Select-Object -First 1 | ForEach-Object { $paths += $_.FullName }
        }
    }

    foreach ($p in $paths) {
        if (-not [string]::IsNullOrWhiteSpace("$p") -and (Test-Path $p -PathType Leaf)) {
            return $p
        }
    }
    return $null
}

function Test-SystemCleanerInstalled {
    $exe = Find-SystemCleanerExe
    if ($exe) {
        $version = try { (Get-Item $exe).VersionInfo.ProductVersion } catch { $null }
        return @{ installed = $true; version = $version; path = $exe }
    }
    return @{ installed = $false; version = $null }
}

function Test-InstantSearchInstalled {
    $candidates = @(
        "$env:ProgramFiles\Everything\es.exe",
        "${env:ProgramFiles(x86)}\Everything\es.exe",
        "C:\Tools\es.exe",
        "$env:LOCALAPPDATA\Microsoft\WinGet\Links\es.exe",
        "$env:ProgramFiles\WinGet\Links\es.exe",
        "$env:ProgramData\Microsoft\WinGet\Links\es.exe",
        "$env:LOCALAPPDATA\Programs\Everything\es.exe",
        "$env:LOCALAPPDATA\Everything\es.exe",
        "$env:APPDATA\Everything\es.exe"
    )
    # Also expand %ProgramFiles% / %ProgramFiles(x86)% / %ProgramW6432% env vars
    foreach ($ev in @($env:ProgramFiles, ${env:ProgramFiles(x86)}, $env:ProgramW6432)) {
        if ($ev) { $candidates += "$ev\Everything\es.exe" }
    }
    foreach ($p in $candidates) {
        if ($p -and (Test-Path $p)) {
            $version = try { (Get-Item $p).VersionInfo.ProductVersion } catch { $null }
            return @{ installed = $true; version = $version }
        }
    }
    # WinGet packages directory (Voidtools.Everything.Cli installs here)
    $wingetPackageRoots = @(
        "$env:LOCALAPPDATA\Microsoft\WinGet\Packages",
        "$env:ProgramFiles\WinGet\Packages",
        "$env:ProgramData\Microsoft\WinGet\Packages"
    )
    foreach ($wingetPkgs in $wingetPackageRoots) {
        if (Test-Path $wingetPkgs -ErrorAction SilentlyContinue) {
            $dirs = Get-ChildItem -Path $wingetPkgs -Directory -ErrorAction SilentlyContinue |
                    Where-Object { $_.Name -like "Voidtools.Everything*" }
            foreach ($d in $dirs) {
                $hit = Get-ChildItem -Path $d.FullName -Filter "es.exe" -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
                if ($hit) {
                    $version = try { $hit.VersionInfo.ProductVersion } catch { $null }
                    return @{ installed = $true; version = $version }
                }
            }
        }
    }

    # %PATH% fallback — catches any install that added es.exe to PATH
    $found = Get-Command es.exe -ErrorAction SilentlyContinue
    if ($found) {
        $version = try { (Get-Item $found.Source).VersionInfo.ProductVersion } catch { $null }
        return @{ installed = $true; version = $version }
    }
    return @{ installed = $false; version = $null }
}

function Test-DiskHealthEngineInstalled {
    $candidates = @(
        "$env:ProgramFiles\smartmontools\bin\smartctl.exe",
        "$env:ProgramFiles\smartmontools\smartctl.exe",
        "C:\ProgramData\chocolatey\bin\smartctl.exe",
        "$env:LOCALAPPDATA\Microsoft\WinGet\Links\smartctl.exe"
    )

    foreach ($p in $candidates) {
        if ($p -and (Test-Path $p)) {
            $version = $null
            try {
                $verOut = & $p --version 2>$null | Select-Object -First 1
                if ($verOut) {
                    $m = [regex]::Match($verOut, '(\d+(?:\.\d+){1,3})')
                    if ($m.Success) { $version = $m.Groups[1].Value }
                }
            }
            catch {}
            return @{ installed = $true; version = $version }
        }
    }

    $cmd = Get-Command smartctl.exe -ErrorAction SilentlyContinue
    if ($cmd) {
        $version = $null
        try {
            $verOut = & $cmd.Source --version 2>$null | Select-Object -First 1
            if ($verOut) {
                $m = [regex]::Match($verOut, '(\d+(?:\.\d+){1,3})')
                if ($m.Success) { $version = $m.Groups[1].Value }
            }
        }
        catch {}
        return @{ installed = $true; version = $version }
    }

    return @{ installed = $false; version = $null }
}

# Locates `exiftool.exe` for both the dep probe AND the scrubber runtime
# (which shells out to it). Returns a full path or $null. Search order:
#   1. PATH (`Get-Command`) — picks up winget link + user installs
#   2. OliverBetz installer dirs under Program Files
#   3. WC's bundled tools dir (future bundle path)
function Resolve-ExifToolExe {
    # Refresh PATH to catch an install that just ran in another process (see Find-Python312Exe)
    $env:Path = [System.Environment]::GetEnvironmentVariable("Path", "Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path", "User")

    $cmd = Get-Command exiftool.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $cmd = Get-Command exiftool -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $candidates = @(
        "$env:ProgramFiles\ExifTool\exiftool.exe",
        "${env:ProgramFiles(x86)}\ExifTool\exiftool.exe",
        "$env:ProgramFiles\exiftool\exiftool.exe",
        "$env:LOCALAPPDATA\Microsoft\WinGet\Links\exiftool.exe",
        "$env:ProgramFiles\WinGet\Links\exiftool.exe",
        "$env:APPDATA\WinCommander\tools\exiftool.exe",
        "$env:LOCALAPPDATA\Programs\ExifTool\exiftool.exe"
    )
    foreach ($p in $candidates) {
        if ($p -and (Test-Path $p)) { return $p }
    }
    foreach ($base in @("$env:LOCALAPPDATA\Microsoft\WinGet\Packages", "$env:ProgramFiles\WinGet\Packages")) {
        if (Test-Path $base) {
            $hit = Get-ChildItem -Path $base -Filter 'exiftool.exe' -Recurse -Depth 2 -ErrorAction SilentlyContinue | Select-Object -First 1
            if ($hit) { return $hit.FullName }
        }
    }
    return $null
}

function Test-MetadataScrubberInstalled {
    $exe = Resolve-ExifToolExe
    if (-not $exe) { return @{ installed = $false; version = $null } }
    $version = $null
    try {
        $verOut = & $exe -ver 2>$null
        if ($verOut) { $version = ($verOut -join '').Trim() }
    } catch {}
    return @{ installed = $true; version = $version }
}

# Locates `ollama.exe` for the Local AI Advisor dep probe. winget drops it
# in a couple of layouts depending on machine- vs user-scope.
function Resolve-OllamaExe {
    $cmd = Get-Command ollama.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    foreach ($p in @(
        "$env:LOCALAPPDATA\Programs\Ollama\ollama.exe",
        "$env:ProgramFiles\Ollama\ollama.exe",
        "$env:LOCALAPPDATA\Microsoft\WinGet\Links\ollama.exe"
    )) {
        if ($p -and (Test-Path $p)) { return $p }
    }
    return $null
}

function Test-LocalLlmInstalled {
    # Installed = ollama.exe present. Running = server answers on 11434.
    # ModelReady (a separate panel concern) = the configured model is in
    # `ollama list`; the dep probe only reports installed/running because
    # "installed" != "ready" and the panel resolves model state via the
    # paid Get-OllamaStatus handler.
    $exe = Resolve-OllamaExe
    $installed = $null -ne $exe
    $version = $null
    $running = $false
    if ($installed) {
        $verOut = try { (& $exe --version 2>&1 | Out-String) } catch { $null }
        $version = $null
        if ($verOut) {
            $m = [regex]::Match($verOut, '(\d+(?:\.\d+){1,3})')
            if ($m.Success) { $version = $m.Groups[1].Value }
        }
        # Cheap liveness probe — short timeout so a stopped server doesn't
        # hang the dep scan.
        try {
            $r = Invoke-WebRequest -Uri 'http://127.0.0.1:11434/api/version' `
                 -UseBasicParsing -TimeoutSec 2 -ErrorAction Stop
            $running = ($r.StatusCode -eq 200)
        } catch { $running = $false }
    }
    return @{ installed = $installed; version = $version; running = $running }
}


# ════════════════════════════════════════════════════════════════════════════
# PER-DEPENDENCY: INSTALL
# ════════════════════════════════════════════════════════════════════════════

function Install-EncryptionEngine {
    Assert-IsAdmin
    $status = Test-EncryptionEngineInstalled
    if ($status.installed) { return @{ success = $true; message = "Encryption Engine already installed." } }

    $wingetCmd = Resolve-WingetPath
    if (-not $wingetCmd) { throw "Winget is required to install Encryption Engine." }

    & $wingetCmd install --id IDRIX.VeraCrypt --exact --silent --accept-source-agreements --accept-package-agreements --force --disable-interactivity
    if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne -1978335212) {
        throw "Failed to install VeraCrypt (exit code $LASTEXITCODE)"
    }

    return @{ success = $true; message = "Encryption Engine installed." }
}

function Test-RamDiskEngineInstalled {
    # Mirrors Get-ImDiskExe in vault/ramdisks.ps1. We duplicate detection
    # here so the dependency status probe doesn't need to load the RAM-disk
    # module (separate AES-decrypt step).
    $cmd = Get-Command imdisk.exe -ErrorAction SilentlyContinue
    if ($cmd) { return @{ installed = $true; version = $null; path = $cmd.Source } }

    foreach ($p in @(
        "${env:ProgramW6432}\ImDisk\imdisk.exe",
        "${env:ProgramFiles}\ImDisk\imdisk.exe",
        "${env:ProgramFiles(x86)}\ImDisk\imdisk.exe",
        "${env:SystemRoot}\System32\imdisk.exe"
    )) {
        if ($p -and (Test-Path $p)) { return @{ installed = $true; version = $null; path = $p } }
    }
    return @{ installed = $false; version = $null; path = $null }
}

function Install-RamDiskEngineDep {
    # Dependency-orchestrator install path. Mirrors vault/ramdisks.ps1::
    # Install-RamDiskEngine. See that function for the comment trail on
    # why we use the IExpress extract-and-run-install.cmd flow rather than
    # launching the installer GUI.
    Assert-IsAdmin
    $status = Test-RamDiskEngineInstalled
    if ($status.installed) { return @{ success = $true; message = 'RAM Disk Engine already installed.' } }

    $urls = @(
        'https://www.ltr-data.se/files/imdiskinst.exe',
        'http://www.ltr-data.se/files/imdiskinst.exe'
    )
    $tmpExe = Join-Path $env:TEMP "rde_$([Guid]::NewGuid().ToString('N').Substring(0,8)).exe"
    $tmpDir = Join-Path $env:TEMP "rde_x_$([Guid]::NewGuid().ToString('N').Substring(0,8))"

    try {
        [Net.ServicePointManager]::SecurityProtocol = `
            [Net.SecurityProtocolType]::Tls12 -bor [Net.SecurityProtocolType]::Tls13
    } catch {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    }

    $downloaded = $false
    $lastError = $null

    foreach ($url in $urls) {
        if (Test-Path $tmpExe) { Remove-Item $tmpExe -Force -ErrorAction SilentlyContinue }
        try {
            $oldPref = $ProgressPreference
            $ProgressPreference = 'SilentlyContinue'
            try {
                Invoke-WebRequest -Uri $url -OutFile $tmpExe -UseBasicParsing `
                    -MaximumRedirection 15 -UserAgent 'Wget/1.21.3' -TimeoutSec 600 -ErrorAction Stop
            } finally { $ProgressPreference = $oldPref }
        } catch { $lastError = $_.Exception.Message }

        if (-not (Test-Path $tmpExe) -or (Get-Item $tmpExe).Length -lt 100000) {
            try {
                if (Test-Path $tmpExe) { Remove-Item $tmpExe -Force -ErrorAction SilentlyContinue }
                Import-Module BitsTransfer -ErrorAction Stop
                Start-BitsTransfer -Source $url -Destination $tmpExe -ErrorAction Stop
            } catch { $lastError = $_.Exception.Message; continue }
        }

        if (-not (Test-Path $tmpExe) -or (Get-Item $tmpExe).Length -lt 100000) { continue }
        try {
            $fs = [System.IO.File]::OpenRead($tmpExe); $b0 = $fs.ReadByte(); $b1 = $fs.ReadByte(); $fs.Close()
            if ($b0 -ne 0x4D -or $b1 -ne 0x5A) { $lastError = 'Not a valid executable.'; continue }
        } catch { $lastError = $_.Exception.Message; continue }

        $downloaded = $true; break
    }

    if (-not $downloaded) {
        if (Test-Path $tmpExe) { Remove-Item $tmpExe -Force -ErrorAction SilentlyContinue }
        throw "Failed to download RAM Disk Engine installer.$(if ($lastError) { ' (' + $lastError + ')' })"
    }

    try {
        Unblock-File -LiteralPath $tmpExe -ErrorAction SilentlyContinue
        $adsPath = $tmpExe + ':Zone.Identifier'
        if (Test-Path -LiteralPath $adsPath) {
            Remove-Item -LiteralPath $adsPath -Force -ErrorAction SilentlyContinue
        }
    } catch {}

    # IExpress extract-only flow.
    New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null
    $silentOk = $false
    try {
        $extractProc = Start-Process -FilePath $tmpExe `
            -ArgumentList @("/T:$tmpDir", '/C', '/Q') `
            -Wait -PassThru -WindowStyle Hidden -ErrorAction Stop
        if ($extractProc.ExitCode -eq 0 -and (Test-Path (Join-Path $tmpDir 'install.cmd'))) {
            $installCmd = Join-Path $tmpDir 'install.cmd'
            $cmdProc = Start-Process -FilePath 'cmd.exe' `
                -ArgumentList @('/c', "`"$installCmd`"", '/64', '/hide') `
                -WorkingDirectory $tmpDir -Wait -PassThru -WindowStyle Hidden -ErrorAction Stop
            if ($cmdProc.ExitCode -eq 0) { $silentOk = $true }
        }
    } catch {}

    if (Test-Path $tmpDir) { Remove-Item $tmpDir -Recurse -Force -ErrorAction SilentlyContinue }

    # Fallback: launch installer GUI (single small dialog).
    if (-not $silentOk) {
        try {
            $proc = Start-Process -FilePath $tmpExe -Wait -PassThru -ErrorAction Stop
            if ($proc.ExitCode -ne 0) {
                Remove-Item $tmpExe -Force -ErrorAction SilentlyContinue
                throw "Installer exited with code $($proc.ExitCode)"
            }
        } catch {
            Remove-Item $tmpExe -Force -ErrorAction SilentlyContinue
            throw "Installer could not start: $($_.Exception.Message)"
        }
    }

    Remove-Item $tmpExe -Force -ErrorAction SilentlyContinue

    # Driver registration takes a moment after the installer exits. Poll
    # for up to 6 s (20 × 300 ms) — mirrors Install-RamDiskEngine in
    # vault/ramdisks.ps1 so both paths behave identically.
    for ($i = 0; $i -lt 20; $i++) {
        if ((Test-RamDiskEngineInstalled).installed) {
            return @{ success = $true; message = 'RAM Disk Engine installed.' }
        }
        Start-Sleep -Milliseconds 300
    }

    throw 'Installer finished but engine binary was not detected. A restart may be required.'
}

function Install-MeshVpn {
    Assert-IsAdmin
    $status = Test-MeshVpnInstalled
    if ($status.installed) { return @{ success = $true; message = "Mesh VPN already installed." } }

    $wingetCmd = Resolve-WingetPath
    if (-not $wingetCmd) { throw "Winget is required to install Mesh VPN." }

    & $wingetCmd install --id Tailscale.Tailscale --exact --silent --accept-source-agreements --accept-package-agreements --force --disable-interactivity
    if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne -1978335212) {
        throw "Failed to install Mesh VPN (exit code $LASTEXITCODE)"
    }

    return @{ success = $true; message = "Mesh VPN installed." }
}

function Install-ProductivityEngine {
    Assert-IsAdmin
    $status = Test-ProductivityEngineInstalled
    if ($status.installed) { return @{ success = $true; message = "Productivity Engine already installed." } }

    $wingetCmd = Resolve-WingetPath
    if (-not $wingetCmd) { throw "Winget is required to install Productivity Engine." }

    & $wingetCmd install --id ActivityWatch.ActivityWatch --exact --silent --accept-source-agreements --accept-package-agreements --force --disable-interactivity
    if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne -1978335212) {
        throw "Failed to install ActivityWatch (exit code $LASTEXITCODE)"
    }

    return @{ success = $true; message = "Productivity Engine installed." }
}

function Install-WingetDependency {
    Assert-IsAdmin
    $status = Test-WingetDependencyInstalled
    if ($status.installed) { return @{ success = $true; message = "Winget already installed." } }

    # Use Install-Winget from apps/winget module if available
    if (Get-Command "Install-Winget" -ErrorAction SilentlyContinue) {
        $result = Install-Winget
        return @{ success = $true; message = "Winget installed." }
    }

    # Fallback: manual install
    Set-PSRepository -Name PSGallery -InstallationPolicy Trusted -ErrorAction SilentlyContinue
    Install-PackageProvider -Name NuGet -Force -ErrorAction SilentlyContinue
    Install-Module Microsoft.WinGet.Client -Force -ErrorAction SilentlyContinue
    Import-Module Microsoft.WinGet.Client -ErrorAction SilentlyContinue
    Repair-WinGetPackageManager -ErrorAction SilentlyContinue

    return @{ success = $true; message = "Winget installed." }
}

function Install-Chocolatey {
    Assert-IsAdmin
    $status = Test-ChocolateyInstalled
    if ($status.installed) { return @{ success = $true; message = "Chocolatey already installed." } }

    try {
        Set-ExecutionPolicy Bypass -Scope Process -Force -ErrorAction SilentlyContinue
        [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor 3072
        Invoke-Expression ((New-Object System.Net.WebClient).DownloadString('https://community.chocolatey.org/install.ps1'))
    } catch {
        throw "Failed to install Chocolatey: $($_.Exception.Message)"
    }

    $status = Test-ChocolateyInstalled
    if (-not $status.installed) {
        throw "Chocolatey installer finished but choco.exe was not detected."
    }
    return @{ success = $true; message = "Chocolatey installed." }
}

function Install-Scoop {
    $status = Test-ScoopInstalled
    if ($status.installed) { return @{ success = $true; message = "Scoop already installed." } }

    [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor 3072
    $tmpScript = Join-Path $env:TEMP "scoop_install_$([Guid]::NewGuid().ToString('N').Substring(0,8)).ps1"

    try {
        # Download to disk and Unblock-File it instead of Invoke-RestMethod +
        # Invoke-Expression on an in-memory string: this app runs elevated, and a
        # remote script piped straight into Invoke-Expression with no
        # Mark-of-the-Web handling is exactly the shape AppLocker/SmartScreen/AV
        # products flag and block.
        Invoke-WebRequest -Uri 'https://get.scoop.sh' -OutFile $tmpScript -UseBasicParsing -ErrorAction Stop
        Unblock-File -LiteralPath $tmpScript -ErrorAction SilentlyContinue

        # Process-scoped so it doesn't persist/affect anything outside this call.
        Set-ExecutionPolicy Bypass -Scope Process -Force -ErrorAction SilentlyContinue

        # Scoop's own installer refuses to run as admin unless told to via
        # -RunAsAdmin (this app runs elevated / requireAdministrator).
        & $tmpScript -RunAsAdmin
    } catch {
        throw "Failed to install Scoop: $($_.Exception.Message)"
    } finally {
        Remove-Item -LiteralPath $tmpScript -Force -ErrorAction SilentlyContinue
    }

    $status = Test-ScoopInstalled
    if (-not $status.installed) {
        throw "Scoop installer finished but scoop.cmd was not detected."
    }
    return @{ success = $true; message = "Scoop installed." }
}

function Install-PowerShell7 {
    Assert-IsAdmin
    $status = Test-PowerShell7Installed
    if ($status.installed) { return @{ success = $true; message = "PowerShell 7 already installed." } }

    $wingetCmd = Resolve-WingetPath
    if (-not $wingetCmd) { throw "Winget is required to install PowerShell 7." }

    & $wingetCmd install --id Microsoft.PowerShell --exact --silent --accept-source-agreements --accept-package-agreements --force --disable-interactivity
    if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne -1978335212) {
        throw "Failed to install PowerShell 7 (exit code $LASTEXITCODE)"
    }

    return @{ success = $true; message = "PowerShell 7 installed." }
}

function Install-VCRedist {
    Assert-IsAdmin
    $wingetCmd = Resolve-WingetPath
    if (-not $wingetCmd) { throw "Winget is required to install Visual C++ Redistributables." }

    $packages = @(
        'Microsoft.VCRedist.2015+.x64',
        'Microsoft.VCRedist.2015+.x86',
        'Microsoft.VCRedist.2013.x64',
        'Microsoft.VCRedist.2013.x86',
        'Microsoft.VCRedist.2012.x64',
        'Microsoft.VCRedist.2012.x86'
    )

    foreach ($pkg in $packages) {
        & $wingetCmd install --id $pkg --exact --silent --accept-source-agreements --accept-package-agreements --force --disable-interactivity
        # Ignore individual failures (some may already be installed or unavailable)
    }

    return @{ success = $true; message = "Visual C++ Redistributables installed." }
}

function Install-InstantSearch {
    Assert-IsAdmin
    $status = Test-InstantSearchInstalled
    if ($status.installed) { return @{ success = $true; message = "Instant Search Engine already installed." } }

    $wingetCmd = Resolve-WingetPath
    if (-not $wingetCmd) { throw "Winget is required to install the Instant Search Engine." }

    # Proactively sync sources — same robustness as Install-WingetApps to avoid -1978335138
    Invoke-WingetSourceUpdate -WingetCmd $wingetCmd

    foreach ($pkgId in @('voidtools.Everything', 'voidtools.Everything.Cli')) {
        & $wingetCmd install --id $pkgId --exact --silent --source winget --accept-source-agreements --accept-package-agreements --force --disable-interactivity
        $code = $LASTEXITCODE

        if ($code -eq 0 -or $code -eq -1978335212) { continue }

        if ($code -eq -1978335231) {
            # Hash mismatch — retry with --ignore-security-hash (safe after source update)
            & $wingetCmd install --id $pkgId --exact --silent --source winget --accept-source-agreements --accept-package-agreements --force --disable-interactivity --ignore-security-hash
            $retryCode = $LASTEXITCODE
            if ($retryCode -eq 0 -or $retryCode -eq -1978335212) { continue }
            throw "Installer hash mismatch for $pkgId and retry also failed (exit code $retryCode)"
        }

        throw "Failed to install $pkgId (exit code $code)"
    }

    return @{ success = $true; message = "Instant Search Engine and CLI installed." }
}

function Install-SystemCleaner {
    Assert-IsAdmin
    $status = Test-SystemCleanerInstalled
    if ($status.installed) { return @{ success = $true; message = "System Cleaner already installed." } }

    $wingetCmd = Resolve-WingetPath
    if (-not $wingetCmd) { throw "Winget is required to install System Cleaner." }

    Invoke-WingetSourceUpdate -WingetCmd $wingetCmd
    & $wingetCmd install --id BleachBit.BleachBit --exact --silent --source winget --accept-source-agreements --accept-package-agreements --force --disable-interactivity
    $code = $LASTEXITCODE
    if ($code -eq -1978335231) {
        & $wingetCmd install --id BleachBit.BleachBit --exact --silent --source winget --accept-source-agreements --accept-package-agreements --force --disable-interactivity --ignore-security-hash
        $code = $LASTEXITCODE
    }
    if ($code -ne 0 -and $code -ne -1978335212) {
        throw "Failed to install System Cleaner (exit code $code)"
    }

    $status = Test-SystemCleanerInstalled
    if (-not $status.installed) {
        throw "Installer finished but System Cleaner was not detected. Re-run engine refresh or install BleachBit manually."
    }

    # Enable winapp2.ini — BleachBit ships with it but it's off by default.
    # The config file is in %APPDATA%\BleachBit\BleachBit.ini
    $bbIni = "$env:APPDATA\BleachBit\BleachBit.ini"
    $bbDir = Split-Path $bbIni
    if (-not (Test-Path $bbDir)) { New-Item -ItemType Directory -Path $bbDir -Force | Out-Null }
    if (Test-Path $bbIni) {
        $content = Get-Content $bbIni -Raw
        if ($content -notmatch 'update_winapp2') {
            Add-Content -Path $bbIni -Value "`nupdate_winapp2 = True"
        }
        else {
            $content = $content -replace 'update_winapp2\s*=\s*False', 'update_winapp2 = True'
            Set-Content -Path $bbIni -Value $content -Encoding UTF8
        }
    }
    else {
        Set-Content -Path $bbIni -Value "[bleachbit]`nupdate_winapp2 = True" -Encoding UTF8
    }

    return @{ success = $true; message = "System Cleaner installed with winapp2.ini enabled." }
}

function Install-DiskHealthEngine {
    Assert-IsAdmin
    $status = Test-DiskHealthEngineInstalled
    if ($status.installed) { return @{ success = $true; message = "Disk Health Engine already installed." } }

    $wingetCmd = Resolve-WingetPath
    if (-not $wingetCmd) { throw "Winget is required to install Disk Health Engine." }

    & $wingetCmd install --id smartmontools.smartmontools --exact --silent --accept-source-agreements --accept-package-agreements --force --disable-interactivity
    if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne -1978335212) {
        throw "Failed to install Disk Health Engine (exit code $LASTEXITCODE)"
    }

    return @{ success = $true; message = "Disk Health Engine installed." }
}

function Install-MetadataScrubber {
    # Doesn't strictly need admin — winget can install to per-user — but
    # forwarding the admin check keeps the install path symmetric with
    # the other winget-backed deps + avoids partial installs on locked
    # boxes.
    Assert-IsAdmin
    $status = Test-MetadataScrubberInstalled
    if ($status.installed) { return @{ success = $true; message = "Hidden Data Remover already installed." } }

    $wingetCmd = Resolve-WingetPath
    if (-not $wingetCmd) { throw "Winget is required to install the Hidden Data Remover." }

    & $wingetCmd install --id OliverBetz.ExifTool --exact --silent --accept-source-agreements --accept-package-agreements --force --disable-interactivity
    if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne -1978335212) {
        throw "Failed to install Hidden Data Remover (winget exit code $LASTEXITCODE)"
    }

    return @{ success = $true; message = "Hidden Data Remover installed." }
}

function Install-LocalLlm {
    Assert-IsAdmin
    $status = Test-LocalLlmInstalled
    if (-not $status.installed) {
        $wingetCmd = Resolve-WingetPath
        if (-not $wingetCmd) { throw "Winget is required to install the Local AI Advisor." }
        & $wingetCmd install --id Ollama.Ollama --exact --silent `
            --accept-source-agreements --accept-package-agreements --force --disable-interactivity
        if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne -1978335212) {
            throw "Failed to install Local AI Advisor (winget exit code $LASTEXITCODE)"
        }
    }
    # NOTE: the model pull is NOT done here. Pulling is a long,
    # progress-bearing operation surfaced in the AI Advisor panel via the
    # paid Pull-OllamaModel Pro handler so the user sees download progress
    # and chooses the model. Install = engine only.
    #
    # The default model the panel pulls is qwen3.5:4b (Qwen3.5 generation —
    # Ollama tags it qwen3.5:*). The allowlist/default live in useAdvisor.ts
    # + llm.rs + settings; this module only installs the engine, so there is
    # no tag literal to bump here when the default model changes.
    return @{ success = $true; message = "Local AI Advisor installed. Pull a model (default qwen3.5:4b) from the AI Advisor panel." }
}

function Install-PrivacyShieldAI {
    param([string]$Target = $null)

    function Install-AIPipPackage {
        param(
            [Parameter(Mandatory = $true)][string]$Spec,
            [switch]$Force
        )
        Write-Host "Installing $Spec..."
        $pipArgs = @("-m", "pip", "install", $Spec, "--quiet", "--no-warn-script-location")
        if ($Force) { $pipArgs += @("--upgrade", "--force-reinstall", "--no-cache-dir") }
        else { $pipArgs += "--upgrade" }
        & $pythonExe @pipArgs *>$null
        if ($LASTEXITCODE -ne 0) {
            throw "Failed to install Privacy Shield AI package $Spec (exit code $LASTEXITCODE)."
        }
    }

    function Test-AIPackageImport {
        param(
            [Parameter(Mandatory = $true)][string]$Package,
            [Parameter(Mandatory = $true)][string]$ImportName
        )

        $script = "import $ImportName"
        $output = & $pythonExe -c $script 2>&1 | Out-String
        return @{
            ok      = ($LASTEXITCODE -eq 0)
            package = $Package
            output  = ($output.Trim())
        }
    }

    function Repair-MediaPipe {
        Write-Host "Repairing MediaPipe runtime..."

        # Remove ABI-sensitive packages first. Leaving a stale NumPy/OpenCV/protobuf
        # wheel in place is the common reason `pip install mediapipe` succeeds but
        # `import mediapipe` still fails.
        & $pythonExe -m pip uninstall -y mediapipe opencv-contrib-python opencv-python protobuf numpy *>$null

        # KT: --force-reinstall (not --no-cache-dir) is enough to fix the ABI
        # mismatch this function exists for -- a stale numpy/protobuf/mediapipe
        # install left in a broken combination. --no-cache-dir additionally
        # forced pip to re-download the same wheels from the network on every
        # single retry instead of reusing what it already fetched, turning a
        # slow install into one that never seemed to make progress on repeat
        # attempts.
        & $pythonExe -m pip install "numpy<2" "protobuf<5" "mediapipe" --upgrade --force-reinstall --quiet --no-warn-script-location *>$null
        if ($LASTEXITCODE -ne 0) {
            throw "Failed to repair Privacy Shield ML Engine (pip exit code $LASTEXITCODE)."
        }

        $probe = Test-AIPackageImport -Package "mediapipe" -ImportName "mediapipe"
        if (-not $probe.ok) {
            $detail = if ($probe.output) { $probe.output } else { "No Python import details were returned." }
            throw "ML Engine import failed after repair: $detail"
        }
    }

    # Step 1: Ensure Python is installed
    $pythonExe = Find-Python312Exe

    if (-not $pythonExe) {
        Write-Host "Python 3.12 not found. Installing via winget..."
        $wingetCmd = Get-LocalWingetPath
        if (-not $wingetCmd) { throw "Winget is required to install Python." }

        & $wingetCmd install --id Python.Python.3.12 --exact --silent --accept-source-agreements --accept-package-agreements --force --disable-interactivity
        if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne -1978335212) {
            throw "Python 3.12 installation failed (exit code $LASTEXITCODE)."
        }
        Start-Sleep -Seconds 5
        $pythonExe = Find-Python312Exe
    }

    if (-not $pythonExe) {
        return @{ error = $true; message = "Python could not be installed. Please install manually from Packages & Apps." }
    }

    if ($Target -eq "Python") {
        return @{ success = $true; message = "Python installed at $pythonExe" }
    }

    # Step 2: Install pip packages
    Write-Host "Ensuring pip..."
    & $pythonExe -m ensurepip --default-pip *>$null
    if ($LASTEXITCODE -ne 0) { throw "Failed to enable pip for Privacy Shield AI (exit code $LASTEXITCODE)." }

    & $pythonExe -m pip install --upgrade pip --quiet *>$null
    if ($LASTEXITCODE -ne 0) { throw "Failed to update pip for Privacy Shield AI (exit code $LASTEXITCODE)." }

    $allPackages = @("mediapipe", "opencv-python", "PyQt6", "numpy", "Pillow")
    if ($Target -and $allPackages -contains $Target) {
        $packages = @($Target)
    }
    else {
        # KT: only (re)install what's actually missing/broken instead of always
        # blowing away and re-downloading all 5 packages. Repair-MediaPipe's
        # --force-reinstall --no-cache-dir made every "install the one missing
        # dependency" call redo a full, cache-bypassing download of mediapipe +
        # numpy + protobuf even when they were already fine — the biggest
        # reason Privacy Shield installs felt like they never finished.
        $currentlyMissing = (Test-PrivacyShieldAIInstalled).missing
        $packages = @($allPackages | Where-Object { $currentlyMissing -contains $_ })
        if ($packages.Count -eq 0) {
            return @{ success = $true; message = "Privacy Shield AI dependencies already installed." }
        }
    }

    # MediaPipe and PyQt ship native wheels; missing VC++ runtime can make the
    # pip install succeed but the import verification fail with DLL load errors.
    $needsMediaPipe = ($packages -contains "mediapipe") -or ($packages -contains "opencv-python") -or ($packages -contains "PyQt6")
    $vcStatus = Test-VCRedistInstalled
    if ($needsMediaPipe -or -not $vcStatus.installed) {
        $vcResult = Install-VCRedist
        if ($vcResult.error) { throw $vcResult.message }
    }

    foreach ($pkg in $packages) {
        if ($pkg -eq "mediapipe") {
            Repair-MediaPipe
        }
        elseif ($pkg -eq "numpy") {
            Install-AIPipPackage "numpy<2"
        }
        else {
            Install-AIPipPackage $pkg
        }
    }

    $verify = Test-PrivacyShieldAIInstalled
    if ($Target -and $packages -contains $Target) {
        if ($verify.missing -contains $Target) {
            $importName = if ($Target -eq "opencv-python") { "cv2" } elseif ($Target -eq "Pillow") { "PIL" } else { $Target.Split('-')[0] }
            $probe = Test-AIPackageImport -Package $Target -ImportName $importName
            $detail = if ($probe.output) { $probe.output } else { "No Python import details were returned." }
            throw "Privacy Shield AI verification failed for $Target. $detail"
        }
    }
    elseif (-not $verify.installed) {
        throw "Privacy Shield AI verification failed. Missing: $($verify.missing -join ', ')"
    }

    return @{ success = $true; message = "Privacy Shield AI dependencies installed." }
}


# ════════════════════════════════════════════════════════════════════════════
# PER-DEPENDENCY: HIDE (remove shortcuts, registry entries, tray icons)
# ════════════════════════════════════════════════════════════════════════════

function Hide-EncryptionEngine {
    $itemsRemoved = 0
    $errors = @()

    # Start Menu folder
    $veracryptFolder = "C:\ProgramData\Microsoft\Windows\Start Menu\Programs\VeraCrypt 1.26.24"
    if (Test-Path $veracryptFolder) {
        try { Invoke-7Erase -Path $veracryptFolder -Type File; $itemsRemoved++ }
        catch { $errors += "VeraCrypt folder: $($_.Exception.Message)" }
    }

    # Uninstall registry keys
    $veracryptUninstallKeys = @(
        'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{9EBED8F8-BD2F-4561-B5A3-628A8815F51F}',
        'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\{9EBED8F8-BD2F-4561-B5A3-628A8815F51F}'
    )
    foreach ($key in $veracryptUninstallKeys) {
        if (Test-Path $key) {
            try {
                Invoke-7Erase -Path $key -Type RegistryProperty -Name 'DisplayName'
                Invoke-7Erase -Path $key -Type RegistryProperty -Name 'DisplayIcon'
                $itemsRemoved++
            }
            catch { $errors += "VeraCrypt registry: $($_.Exception.Message)" }
        }
    }

    # Desktop shortcuts
    foreach ($desktop in @("$env:PUBLIC\Desktop", "$env:USERPROFILE\Desktop")) {
        $path = Join-Path $desktop "VeraCrypt.lnk"
        if (Test-Path $path) {
            try { Invoke-7Erase -Path $path -Type File; $itemsRemoved++ }
            catch { $errors += "VeraCrypt desktop: $($_.Exception.Message)" }
        }
    }

    return @{ itemsRemoved = $itemsRemoved; warnings = $errors }
}

function Hide-MeshVpn {
    $itemsRemoved = 0
    $errors = @()

    # Start Menu Startup shortcut
    $tailscaleStartup = "C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Startup\Tailscale.lnk"
    if (Test-Path $tailscaleStartup) {
        try { Invoke-7Erase -Path $tailscaleStartup -Type File; $itemsRemoved++ }
        catch { $errors += "Tailscale startup: $($_.Exception.Message)" }
    }

    # Uninstall registry keys
    $tailscaleUninstallKeys = @(
        'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{15FA8713-42EE-5F73-8293-CDA82637B6D5}',
        'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\{15FA8713-42EE-5F73-8293-CDA82637B6D5}'
    )
    foreach ($key in $tailscaleUninstallKeys) {
        if (Test-Path $key) {
            try {
                Invoke-7Erase -Path $key -Type RegistryProperty -Name 'DisplayName'
                Invoke-7Erase -Path $key -Type RegistryProperty -Name 'DisplayIcon'
                $itemsRemoved++
            }
            catch { $errors += "Tailscale registry: $($_.Exception.Message)" }
        }
    }

    # Desktop shortcuts
    foreach ($desktop in @("$env:PUBLIC\Desktop", "$env:USERPROFILE\Desktop")) {
        $path = Join-Path $desktop "Tailscale.lnk"
        if (Test-Path $path) {
            try { Invoke-7Erase -Path $path -Type File; $itemsRemoved++ }
            catch { $errors += "Tailscale desktop: $($_.Exception.Message)" }
        }
    }

    # Configure unattended mode and stop UI
    if (Get-Command "Set-MeshVPNConfig" -ErrorAction SilentlyContinue) {
        $status = Get-MeshVPNStatus
        if ($status.installed) {
            $advNode = if ($status.prefs) { $status.prefs.AdvertiseExitNode } else { $false }
            $allowLan = if ($status.prefs) { $status.prefs.ExitNodeAllowLANAccess } else { $false }
            $accRoutes = if ($status.prefs) { $status.prefs.AcceptRoutes } else { $false }
            $exitIp = if ($status.prefs) { $status.prefs.ExitNodeIP } else { "" }
            $sUp = if ($status.prefs) { $status.prefs.ShieldsUp } else { $false }
            $accDns = if ($status.prefs) { $status.prefs.AcceptDNS } else { $false }
            Set-MeshVPNConfig -AdvertiseExitNode $advNode -AllowLanAccess $allowLan -Unattended $true -AcceptRoutes $accRoutes -ExitNodeIP $exitIp -ShieldsUp $sUp -AcceptDNS $accDns -Force:$false | Out-Null
        }
    }

    try {
        Set-Service -Name "Tailscale" -StartupType Automatic -ErrorAction SilentlyContinue
        Start-Service -Name "Tailscale" -ErrorAction SilentlyContinue
    }
    catch {
        $errors += "Tailscale service: $($_.Exception.Message)"
    }

    try { Stop-Process -Name "tailscale-ipn" -Force -ErrorAction SilentlyContinue } catch {}

    return @{ itemsRemoved = $itemsRemoved; warnings = $errors }
}

function Hide-ProductivityEngine {
    $itemsRemoved = 0
    $errors = @()

    # Start Menu shortcut
    $awStartMenu = "C:\ProgramData\Microsoft\Windows\Start Menu\Programs\ActivityWatch.lnk"
    if (Test-Path $awStartMenu) {
        try { Invoke-7Erase -Path $awStartMenu -Type File; $itemsRemoved++ }
        catch { $errors += "ActivityWatch start menu: $($_.Exception.Message)" }
    }

    # Startup shortcut
    $awStartup = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Startup\ActivityWatch.lnk"
    if (Test-Path $awStartup) {
        try { Invoke-7Erase -Path $awStartup -Type File; $itemsRemoved++ }
        catch { $errors += "ActivityWatch startup: $($_.Exception.Message)" }
    }

    # Uninstall registry key (HKCU)
    $awUninstallKey = 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{A18A23E6-901D-0CE8-035423E4}_is1'
    if (Test-Path $awUninstallKey) {
        try {
            Invoke-7Erase -Path $awUninstallKey -Type RegistryProperty -Name 'DisplayName'
            Invoke-7Erase -Path $awUninstallKey -Type RegistryProperty -Name 'DisplayIcon'
            $itemsRemoved++
        }
        catch { $errors += "ActivityWatch registry: $($_.Exception.Message)" }
    }

    # Hide tray icon (kill aw-qt, run watchers headless)
    $awProcesses = Get-Process -Name "aw-qt" -ErrorAction SilentlyContinue
    if ($awProcesses) {
        try {
            Stop-Process -Name "aw-qt" -Force -ErrorAction SilentlyContinue
            # Persist stealth mode state
            if (Get-Command "Set-WCSetting" -EA SilentlyContinue) {
                Set-WCSetting -Path "productivity.productivityEngineStealthEnabled" -Value $true
            }
            # Trigger maintenance to ensure backends are running hidden
            if (Get-Command "Invoke-ProductivityEngineMaintenance" -EA SilentlyContinue) {
                Invoke-ProductivityEngineMaintenance | Out-Null
            }
            $itemsRemoved++
        }
        catch { $errors += "ActivityWatch tray hide: $($_.Exception.Message)" }
    }

    # Patch ActivityWatch HTML to hide navbar/footer
    $awHtmlPath = "$env:LOCALAPPDATA\Programs\ActivityWatch\aw-server\aw_server\static\index.html"
    # Also check Administrator path as fallback
    if (-not (Test-Path $awHtmlPath)) {
        $awHtmlPath = "C:\Users\Administrator\AppData\Local\Programs\ActivityWatch\aw-server\aw_server\static\index.html"
    }
    if (Test-Path $awHtmlPath) {
        try {
            $content = Get-Content -Path $awHtmlPath -Raw
            if ($content -notmatch 'id="wc-hide-style"') {
                $style = '<style id="wc-hide-style">.navbar-expand-lg.navbar-light.aw-navbar.navbar,div.container > .mb-2,.my-2.float-md-left.float-none,.my-2.float-md-right.float-none,footer,.footer,.aw-footer,header,.header,.alert-info.alert{display:none!important}</style>'
                $newContent = $content -replace '</head>', ($style + '</head>')
                Set-Content -Path $awHtmlPath -Value $newContent -Encoding UTF8
                $itemsRemoved++
            }
        }
        catch { $errors += "ActivityWatch HTML patch: $($_.Exception.Message)" }
    }

    # Desktop shortcuts
    foreach ($desktop in @("$env:PUBLIC\Desktop", "$env:USERPROFILE\Desktop")) {
        $path = Join-Path $desktop "ActivityWatch.lnk"
        if (Test-Path $path) {
            try { Invoke-7Erase -Path $path -Type File; $itemsRemoved++ }
            catch { $errors += "ActivityWatch desktop: $($_.Exception.Message)" }
        }
    }

    return @{ itemsRemoved = $itemsRemoved; warnings = $errors }
}

function Hide-SystemCleaner {
    $itemsRemoved = 0
    $errors = @()

    # Start Menu folder
    $bbStartMenu = "C:\ProgramData\Microsoft\Windows\Start Menu\Programs\BleachBit"
    if (Test-Path $bbStartMenu) {
        try { Remove-ItemSecure -Path $bbStartMenu -Recurse -Force -ErrorAction SilentlyContinue; $itemsRemoved++ }
        catch { $errors += "BleachBit start menu: $($_.Exception.Message)" }
    }

    # Desktop shortcuts
    foreach ($desktop in @("$env:PUBLIC\Desktop", "$env:USERPROFILE\Desktop")) {
        foreach ($shortcut in @("BleachBit.lnk", "BleachBit as Administrator.lnk")) {
            $path = Join-Path $desktop $shortcut
            if (Test-Path $path) {
                try { Remove-ItemSecure -Path $path -Force -ErrorAction SilentlyContinue; $itemsRemoved++ }
                catch { $errors += "BleachBit desktop $shortcut : $($_.Exception.Message)" }
            }
        }
    }

    return @{ itemsRemoved = $itemsRemoved; warnings = $errors }
}

function Hide-InstantSearchEngine {
    $itemsRemoved = 0
    $errors = @()

    try {
        $itemsRemoved += Set-EverythingQuietMode -Hidden $true -Warnings ([ref]$errors)
    }
    catch {
        $errors += "Everything quiet mode: $($_.Exception.Message)"
    }

    return @{ itemsRemoved = $itemsRemoved; warnings = $errors }
}

function Hide-UniGetUI {
    # UniGetUI (WinGet GUI) — kept separate since it's not a gated dep but is a backend app
    $itemsRemoved = 0
    $errors = @()

    # User-level uninstall key
    $unigetuiUninstallKey = 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\UniGetUI (Current user)'
    if (Test-Path $unigetuiUninstallKey) {
        try {
            Invoke-7Erase -Path $unigetuiUninstallKey -Type RegistryProperty -Name 'DisplayName'
            Invoke-7Erase -Path $unigetuiUninstallKey -Type RegistryProperty -Name 'DisplayIcon'
            $itemsRemoved++
        }
        catch { $errors += "UniGetUI registry: $($_.Exception.Message)" }
    }

    # User Start Menu
    $unigetuiStartMenu = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\UniGetUI"
    if (Test-Path $unigetuiStartMenu) {
        try { Invoke-7Erase -Path $unigetuiStartMenu -Type File; $itemsRemoved++ }
        catch { $errors += "UniGetUI start menu: $($_.Exception.Message)" }
    }

    # Desktop shortcuts
    foreach ($desktop in @("$env:PUBLIC\Desktop", "$env:USERPROFILE\Desktop")) {
        foreach ($shortcut in @("UniGetUI.lnk", "WingetUI.lnk")) {
            $path = Join-Path $desktop $shortcut
            if (Test-Path $path) {
                try { Invoke-7Erase -Path $path -Type File; $itemsRemoved++ }
                catch { $errors += "UniGetUI desktop $shortcut : $($_.Exception.Message)" }
            }
        }
    }

    return @{ itemsRemoved = $itemsRemoved; warnings = $errors }
}


# ════════════════════════════════════════════════════════════════════════════
# PER-DEPENDENCY: START (ensure service/process is running)
# ════════════════════════════════════════════════════════════════════════════

function Start-MeshVpnService {
    # Tailscale runs as a Windows service — ensure it's started
    try {
        $svc = Get-Service -Name "Tailscale" -ErrorAction SilentlyContinue
        if ($svc -and $svc.Status -ne 'Running') {
            Start-Service -Name "Tailscale" -ErrorAction SilentlyContinue
        }
        return @{ success = $true; message = "Mesh VPN service started." }
    }
    catch {
        return @{ error = $true; message = "Failed to start Tailscale service: $($_.Exception.Message)" }
    }
}

function Start-ProductivityEngineService {
    # Start ActivityWatch server + watchers in background (headless, no window)
    try {
        if (Get-Command "Start-ProductivityTracker" -ErrorAction SilentlyContinue) {
            return Start-ProductivityTracker
        }

        $components = @(
            @{ name = "aw-server"; paths = @(
                "$env:LOCALAPPDATA\Programs\ActivityWatch\aw-server-rust\aw-server.exe",
                "$env:LOCALAPPDATA\Programs\ActivityWatch\aw-server\aw-server.exe",
                "$env:LOCALAPPDATA\Programs\ActivityWatch\aw-server.exe",
                "$env:ProgramFiles\ActivityWatch\aw-server-rust\aw-server.exe",
                "$env:ProgramFiles\ActivityWatch\aw-server\aw-server.exe",
                "$env:ProgramFiles\ActivityWatch\aw-server.exe",
                "${env:ProgramFiles(x86)}\ActivityWatch\aw-server-rust\aw-server.exe",
                "${env:ProgramFiles(x86)}\ActivityWatch\aw-server\aw-server.exe",
                "${env:ProgramFiles(x86)}\ActivityWatch\aw-server.exe"
            ) },
            @{ name = "aw-watcher-afk"; paths = @(
                "$env:LOCALAPPDATA\Programs\ActivityWatch\aw-watcher-afk\aw-watcher-afk.exe",
                "$env:LOCALAPPDATA\Programs\ActivityWatch\aw-watcher-afk.exe",
                "$env:ProgramFiles\ActivityWatch\aw-watcher-afk\aw-watcher-afk.exe",
                "$env:ProgramFiles\ActivityWatch\aw-watcher-afk.exe",
                "${env:ProgramFiles(x86)}\ActivityWatch\aw-watcher-afk\aw-watcher-afk.exe",
                "${env:ProgramFiles(x86)}\ActivityWatch\aw-watcher-afk.exe"
            ) },
            @{ name = "aw-watcher-window"; paths = @(
                "$env:LOCALAPPDATA\Programs\ActivityWatch\aw-watcher-window\aw-watcher-window.exe",
                "$env:LOCALAPPDATA\Programs\ActivityWatch\aw-watcher-window.exe",
                "$env:ProgramFiles\ActivityWatch\aw-watcher-window\aw-watcher-window.exe",
                "$env:ProgramFiles\ActivityWatch\aw-watcher-window.exe",
                "${env:ProgramFiles(x86)}\ActivityWatch\aw-watcher-window\aw-watcher-window.exe",
                "${env:ProgramFiles(x86)}\ActivityWatch\aw-watcher-window.exe"
            ) }
        )

        $started = 0
        $missing = @()
        foreach ($component in $components) {
            if (Get-Process -Name $component.name -ErrorAction SilentlyContinue) { continue }
            $exe = $null
            foreach ($path in $component.paths) {
                if (Test-Path $path -ErrorAction SilentlyContinue) { $exe = $path; break }
            }
            if (-not $exe -and (Get-Command $component.name -ErrorAction SilentlyContinue)) {
                $exe = (Get-Command $component.name).Source
            }
            if (-not $exe) { $missing += $component.name; continue }

            Start-Process -FilePath $exe -WorkingDirectory (Split-Path $exe -Parent) -WindowStyle Hidden -ErrorAction Stop
            $started++
        }

        if ($missing.Count -gt 0) {
            return @{ error = $true; message = "Missing ActivityWatch component(s): $($missing -join ', ')." }
        }

        return @{ success = $true; message = "Productivity Engine headless components ready."; started = $started }
    }
    catch {
        return @{ error = $true; message = "Failed to start Productivity Engine: $($_.Exception.Message)" }
    }
}

function Start-LocalLlmService {
    # winget's Ollama registers a per-user autostart + a service; ensure the
    # server is listening. `ollama serve` is idempotent-ish but errors if
    # already bound, so probe 11434 first and only spawn if nothing answers.
    try {
        $r = Invoke-WebRequest -Uri 'http://127.0.0.1:11434/api/version' -UseBasicParsing -TimeoutSec 2 -ErrorAction Stop
        if ($r.StatusCode -eq 200) { return @{ success = $true; message = "Local AI server already running." } }
    } catch { }
    $exe = Resolve-OllamaExe
    if (-not $exe) { return @{ error = $true; message = "Local AI engine not found." } }
    Start-Process -FilePath $exe -ArgumentList 'serve' -WindowStyle Hidden -ErrorAction SilentlyContinue
    return @{ success = $true; message = "Local AI server starting." }
}


# ════════════════════════════════════════════════════════════════════════════
# ORCHESTRATORS
# ════════════════════════════════════════════════════════════════════════════

function Get-DependencyStatus {
    <#
    .SYNOPSIS
        Returns status of all dependencies in one call.
        Fast: no installations, just filesystem/registry/process checks.
        Two-tier cache: 15-second in-memory (absorbs rapid UI polls)
        and 12-hour file-backed (survives app restarts).
        Pass -Force to bypass both caches and run a fresh probe.
    #>
    param([switch]$Force)

    # ── Tier 1: In-memory 15s cache ────────────────────────────────
    if (-not $Force -and
        (Test-Path Variable:script:_depStatusCache) -and
        (Test-Path Variable:script:_depStatusCacheTime)) {
        $age = ((Get-Date) - $script:_depStatusCacheTime).TotalSeconds
        if ($age -lt 15) { return $script:_depStatusCache }
    }

    # ── Tier 2: File-backed 12hr cache ─────────────────────────────
    if (-not $Force) {
        $cached = Read-DepStatusCache
        if ($cached) {
            $payload = @{ dependencies = $cached.status; cacheAgeSecs = $cached.cacheAgeSecs }
            $script:_depStatusCache     = $payload
            $script:_depStatusCacheTime = Get-Date
            return $payload
        }
    }

    # ── Tier 3: Live probe ──────────────────────────────────────────
    $registry = Get-DependencyRegistry
    $results = @()

    foreach ($dep in $registry) {
        $status = switch ($dep.id) {
            'encryptionEngine' { Test-EncryptionEngineInstalled }
            'ramDiskEngine' { Test-RamDiskEngineInstalled }
            'meshVpn' { Test-MeshVpnInstalled }
            'productivityEngine' { Test-ProductivityEngineInstalled }
            'winget' { Test-WingetDependencyInstalled }
            'chocolatey' { Test-ChocolateyInstalled }
            'scoop' { Test-ScoopInstalled }
            'privacyShieldAI' { Test-PrivacyShieldAIInstalled }
            'powershell7' { Test-PowerShell7Installed }
            'vcredist' { Test-VCRedistInstalled }
            'systemCleaner' { Test-SystemCleanerInstalled }
            'instantSearch' { Test-InstantSearchInstalled }
            'diskHealthEngine' { Test-DiskHealthEngineInstalled }
            'metadataScrubber' { Test-MetadataScrubberInstalled }
            'localLlm' { Test-LocalLlmInstalled }
        }

        $results += @{
            id        = $dep.id
            name      = $dep.name
            panelId   = $dep.panelId
            installed = [bool]$status.installed
            version   = $status.version
            running   = if ($status.ContainsKey('running')) { $status.running } else { $null }
            connected = if ($status.ContainsKey('connected')) { $status.connected } else { $null }
            missing   = if ($status.ContainsKey('missing')) { $status.missing } else { $null }
            canHide   = $dep.canHide
            canStart  = $dep.canStart
        }
    }

    Write-DepStatusCache $results

    $payload = @{ dependencies = $results; cacheAgeSecs = 0 }
    $script:_depStatusCache     = $payload
    $script:_depStatusCacheTime = Get-Date
    return $payload
}

function Install-Dependency {
    <#
    .SYNOPSIS
        Install a single dependency by ID.
        After installing, automatically hides and starts the app if applicable.
    .PARAMETER Id
        One of: encryptionEngine, meshVpn, productivityEngine, winget, chocolatey, scoop,
        privacyShieldAI, powershell7, vcredist, systemCleaner, instantSearch, diskHealthEngine
    .PARAMETER Target
        Optional sub-target (for privacyShieldAI: specific package name)
    #>
    param(
        [Parameter(Mandatory = $true)]
        [string]$Id,
        [string]$Target = $null
    )

    Assert-IsAdmin

    # Bust in-memory cache so next Get-DependencyStatus reflects post-install state.
    Remove-Variable -Scope script -Name _depStatusCache -ErrorAction SilentlyContinue
    Remove-Variable -Scope script -Name _depStatusCacheTime -ErrorAction SilentlyContinue
    # Also drop cached Python interpreter — fresh install may change which exe to use.
    if ($Id -eq 'privacyShieldAI' -or $Id -eq 'productivityEngine') {
        Remove-Variable -Scope script -Name _cachedPython312Exe -ErrorAction SilentlyContinue
    }

    try {
        # Step 1: Install
        $installResult = switch ($Id) {
            'encryptionEngine' { Install-EncryptionEngine }
            'ramDiskEngine' { Install-RamDiskEngineDep }
            'meshVpn' { Install-MeshVpn }
            'productivityEngine' { Install-ProductivityEngine }
            'winget' { Install-WingetDependency }
            'chocolatey' { Install-Chocolatey }
            'scoop' { Install-Scoop }
            'privacyShieldAI' { Install-PrivacyShieldAI -Target $Target }
            'powershell7' { Install-PowerShell7 }
            'vcredist' { Install-VCRedist }
            'systemCleaner' { Install-SystemCleaner }
            'instantSearch' { Install-InstantSearch }
            'diskHealthEngine' { Install-DiskHealthEngine }
            'metadataScrubber' { Install-MetadataScrubber }
            'localLlm' { Install-LocalLlm }
            default { throw "Unknown dependency: $Id" }
        }

        if ($installResult.error) { return $installResult }

        $registry = Get-DependencyRegistry
        $depDef = $registry | Where-Object { $_.id -eq $Id }

        # Step 2: Hide (if applicable)
        $hideResult = $null
        if ($depDef.canHide) {
            $hideResult = Set-BackendAppsVisibility -Apps $Id -Hidden $true
        }

        # Step 3: Start (if applicable)
        $startResult = $null
        if ($depDef.canStart) {
            $startResult = switch ($Id) {
                'meshVpn' { Start-MeshVpnService }
                'productivityEngine' { Start-ProductivityEngineService }
                'localLlm' { Start-LocalLlmService }
            }
        }

        # Update file cache so the next Get-DependencyStatus (even from a cached
        # read) sees the correct post-install state without waiting for a full probe.
        # Also mark running=true when the auto-start step succeeded so the running
        # gate in DependencyGate.tsx clears in the same round-trip.
        $mergeProps = @{ installed = $true }
        if ($startResult -and $startResult.success) { $mergeProps['running'] = $true }
        Update-DepStatusCacheEntry -depId $Id -mergeProps $mergeProps

        return @{
            success = $true
            id      = $Id
            message = $installResult.message
            hidden  = if ($hideResult) { $hideResult.itemsChanged } else { 0 }
            started = if ($startResult -and $startResult.success) { $true } else { $false }
        }
    }
    catch {
        return @{ error = $true; id = $Id; message = $_.Exception.Message }
    }
}

function Install-AllDependencies {
    <#
    .SYNOPSIS
        Install all missing dependencies in sequence.
        Flow per dep: check → install → hide → start
    #>
    Assert-IsAdmin

    # Bust the entire file cache — a batch install changes multiple entries,
    # easier to force a full fresh probe after completion.
    Remove-Item $script:DEP_CACHE_FILE -ErrorAction SilentlyContinue

    $statusResult = Get-DependencyStatus
    $results = @()
    $allOk = $true

    foreach ($dep in $statusResult.dependencies) {
        if ($dep.installed) {
            # Already installed — but maybe not running
            if ($dep.canStart -and $dep.running -eq $false) {
                $startResult = switch ($dep.id) {
                    'meshVpn' { Start-MeshVpnService }
                    'productivityEngine' { Start-ProductivityEngineService }
                    'localLlm' { Start-LocalLlmService }
                }
                $results += @{ id = $dep.id; status = 'started'; message = "Was installed but not running. Started." }
            }
            else {
                $results += @{ id = $dep.id; status = 'already-installed' }
            }
            continue
        }

        # Not installed — full flow: install → hide → start
        $installResult = Install-Dependency -Id $dep.id
        if ($installResult.error) {
            $allOk = $false
            $results += @{ id = $dep.id; status = 'failed'; message = $installResult.message }
        }
        else {
            $results += @{
                id      = $dep.id
                status  = 'installed'
                message = $installResult.message
                hidden  = $installResult.hidden
                started = $installResult.started
            }
        }
    }

    return @{ success = $allOk; results = $results }
}

function Start-DependencyService {
    <#
    .SYNOPSIS
        Start a single already-installed dependency's background service by
        ID (canStart=true deps only). Used by panels that need to bring a
        stopped engine up without re-running the installer — e.g. the AI
        Advisor panel's "Start AI server" CTA when the engine is installed
        but the 11434 server is down.
    #>
    param(
        [Parameter(Mandatory = $true)]
        [string]$Id
    )

    Assert-IsAdmin

    # Bust the in-memory cache so the next Get-DependencyStatus reflects the
    # post-start `running` state.
    Remove-Variable -Scope script -Name _depStatusCache -ErrorAction SilentlyContinue
    Remove-Variable -Scope script -Name _depStatusCacheTime -ErrorAction SilentlyContinue

    try {
        $result = switch ($Id) {
            'meshVpn' { Start-MeshVpnService }
            'productivityEngine' { Start-ProductivityEngineService }
            'localLlm' { Start-LocalLlmService }
            default { @{ error = $true; message = "Dependency '$Id' has no startable service." } }
        }
        # Write running=true into the file cache so the next Get-DependencyStatus
        # returns the correct state without a full live probe.
        if ($result.success) {
            Update-DepStatusCacheEntry -depId $Id -mergeProps @{ running = $true }
        }
        return $result
    }
    catch {
        return @{ error = $true; id = $Id; message = $_.Exception.Message }
    }
}

function Hide-AllBackendApps {
    <#
    .SYNOPSIS
        Hide all backend apps (replaces old monolithic Hide-BackendApps).
        Uses the reversible visibility path.
    #>
    Assert-IsAdmin

    try {
        $result = Set-BackendAppsVisibility -Apps "meshVpn,encryptionEngine,productivityEngine,instantSearch,systemCleaner,unigetui,ramDiskEngine" -Hidden $true
        return @{
            status       = 'hidden'
            itemsRemoved = $result.itemsChanged
            warnings     = $result.warnings
        }
    }
    catch {
        @{ error = $true; message = "Failed to hide backend apps: $($_.Exception.Message)" }
    }
}

function Get-WinCommanderVisibilityStatePath {
    if ([string]::IsNullOrEmpty($env:ProgramData)) { return $null }
    return "$env:ProgramData\WinCommander\visibility_state.json"
}

function Get-WinCommanderMachineStateDir {
    if ([string]::IsNullOrEmpty($env:ProgramData)) { return $null }
    return "$env:ProgramData\WinCommander"
}

function Get-WinCommanderHideFlagPath {
    $dir = Get-WinCommanderMachineStateDir
    if ([string]::IsNullOrEmpty($dir)) { return $null }
    return "$dir\session_state.dat"
}

$WinCommanderHiddenSuffix = "__SystemCache"
$WinCommanderLegacyHiddenSuffix = "__WC_Hidden"
$WinCommanderHiddenPrefix = "__SystemCache_"
$WinCommanderLegacyHiddenPrefix = "__WC_Hidden_"

function Get-WinCommanderStartMenuTargets {
    # Resolve the real Desktop path — on Windows 11 + OneDrive this is often
    # redirected to %OneDrive%\Desktop or %OneDriveConsumer%\Desktop, NOT %USERPROFILE%\Desktop.
    $desktopPaths = [System.Collections.Generic.List[string]]::new()
    try {
        $shell = [Environment]::GetFolderPath('Desktop')
        if (-not [string]::IsNullOrEmpty($shell)) { [void]$desktopPaths.Add($shell) }
    } catch {}
    # Also cover OneDrive-redirected Desktop variants
    foreach ($odVar in @($env:OneDrive, $env:OneDriveConsumer, $env:OneDriveCommercial)) {
        if (-not [string]::IsNullOrEmpty($odVar)) {
            $odDesktop = "$odVar\Desktop"
            if (-not $desktopPaths.Contains($odDesktop)) { [void]$desktopPaths.Add($odDesktop) }
        }
    }
    # Always include the classic path as a fallback
    $classic = "$env:USERPROFILE\Desktop"
    if (-not $desktopPaths.Contains($classic)) { [void]$desktopPaths.Add($classic) }

    $names = @('WinCommander', 'WinCommander Free', 'WinCommander Pro')

    $paths = [System.Collections.Generic.List[string]]::new()

    # Start Menu — ProgramData (machine-wide, needs admin)
    foreach ($n in $names) {
        [void]$paths.Add("C:\ProgramData\Microsoft\Windows\Start Menu\Programs\$n.lnk")
    }
    [void]$paths.Add("C:\ProgramData\Microsoft\Windows\Start Menu\Programs\WinCommander")
    [void]$paths.Add("C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Uninstall WinCommander.lnk")

    # Start Menu — per-user APPDATA
    foreach ($n in $names) {
        [void]$paths.Add("$env:APPDATA\Microsoft\Windows\Start Menu\Programs\$n.lnk")
    }
    [void]$paths.Add("$env:APPDATA\Microsoft\Windows\Start Menu\Programs\WinCommander")

    # Desktop — public (all users)
    foreach ($n in $names) {
        [void]$paths.Add("$env:PUBLIC\Desktop\$n.lnk")
    }

    # Desktop — all resolved user paths (standard + OneDrive-redirected)
    foreach ($dp in $desktopPaths) {
        foreach ($n in $names) {
            [void]$paths.Add("$dp\$n.lnk")
        }
    }

    return $paths | Select-Object -Unique
}

function Get-WinCommanderUninstallKeys {
    $keys = New-Object System.Collections.Generic.List[string]

    $candidates = @(
        'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\WinCommander',
        'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\WinCommander',
        'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\WinCommander'
    )

    foreach ($key in $candidates) {
        if (Test-Path $key) { [void]$keys.Add($key) }
    }

    $roots = @(
        'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
        'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
        'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
    )

    foreach ($root in $roots) {
        if (-not (Test-Path $root)) { continue }
        $subkeys = Get-ChildItem -Path $root -ErrorAction SilentlyContinue
        foreach ($subkey in $subkeys) {
            try {
                $props = Get-ItemProperty -Path $subkey.PSPath -ErrorAction SilentlyContinue
                $displayName = "$($props.DisplayName)"
                $publisher = "$($props.Publisher)"
                if ($displayName -match 'WinCommander' -or $publisher -match 'ServaLabs') {
                    [void]$keys.Add($subkey.PSPath)
                }
            }
            catch {}
        }
    }

    return $keys | Select-Object -Unique
}

function Invoke-ShellChangeNotify {
    # Tells Windows Shell and Search to rebuild app caches immediately.
    try {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class ShellNotify {
    [DllImport("shell32.dll")]
    public static extern void SHChangeNotify(int wEventId, uint uFlags, IntPtr item1, IntPtr item2);
    public const int  SHCNE_ASSOCCHANGED = 0x08000000;  // app associations changed
    public const int  SHCNE_UPDATEDIR    = 0x00001000;  // directory contents changed
    public const uint SHCNF_IDLIST       = 0x0000;
    public const uint SHCNF_PATHW        = 0x0005;      // item1/item2 are wide string paths
    public const uint SHCNF_FLUSH        = 0x1000;      // flush all pending notifications
}
'@ -ErrorAction SilentlyContinue

        # Rebuild the app-association database (affects Search app list and Open-With).
        [ShellNotify]::SHChangeNotify([ShellNotify]::SHCNE_ASSOCCHANGED,
            [ShellNotify]::SHCNF_IDLIST -bor [ShellNotify]::SHCNF_FLUSH,
            [IntPtr]::Zero, [IntPtr]::Zero)

        # Also notify that the Start Menu program folders changed so the indexer re-scans them.
        $startMenuDirs = @(
            [System.Environment]::GetFolderPath('CommonPrograms'),   # C:\ProgramData\Microsoft\Windows\Start Menu\Programs
            [System.Environment]::GetFolderPath('Programs')          # %APPDATA%\Microsoft\Windows\Start Menu\Programs
        ) | Where-Object { -not [string]::IsNullOrEmpty($_) -and (Test-Path $_) }

        foreach ($dir in $startMenuDirs) {
            $ptrDir = [System.Runtime.InteropServices.Marshal]::StringToHGlobalUni($dir)
            try {
                [ShellNotify]::SHChangeNotify([ShellNotify]::SHCNE_UPDATEDIR,
                    [ShellNotify]::SHCNF_PATHW -bor [ShellNotify]::SHCNF_FLUSH,
                    $ptrDir, [IntPtr]::Zero)
            } finally {
                [System.Runtime.InteropServices.Marshal]::FreeHGlobal($ptrDir)
            }
        }
    } catch {}
}

function Get-WinCommanderExePath {
    # Try uninstall keys first (most reliable for installed builds).
    foreach ($key in (Get-WinCommanderUninstallKeys)) {
        try {
            $props = Get-ItemProperty $key -ErrorAction SilentlyContinue
            if ($props.DisplayIcon) {
                $exePath = $props.DisplayIcon -replace ',\s*-?\d+\s*$', ''  # strip ,index suffix
                if ((Test-Path $exePath) -and $exePath -match '\.exe$') { return $exePath }
            }
            if ($props.InstallLocation) {
                foreach ($name in @('WinCommander.exe', 'wincommander-free.exe')) {
                    $c = Join-Path $props.InstallLocation $name
                    if (Test-Path $c) { return $c }
                }
            }
        } catch {}
    }
    # Common install paths as fallback.
    $candidates = @(
        "$env:ProgramFiles\WinCommander\WinCommander.exe",
        "$env:ProgramFiles\WinCommander\wincommander-free.exe",
        "${env:ProgramFiles(x86)}\WinCommander\WinCommander.exe",
        "$env:LocalAppData\WinCommander\WinCommander.exe",
        "$env:LocalAppData\WinCommander\wincommander-free.exe"
    )
    foreach ($c in $candidates) { if (Test-Path $c) { return $c } }
    return $null
}

function Get-WinCommanderAppPaths {
    # App Paths registry keys — Windows Search uses these to find apps by name
    # even when Start Menu shortcuts are gone.
    return @(
        'HKLM:\Software\Microsoft\Windows\CurrentVersion\App Paths\WinCommander.exe',
        'HKLM:\Software\Microsoft\Windows\CurrentVersion\App Paths\wincommander-free.exe',
        'HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths\WinCommander.exe',
        'HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths\wincommander-free.exe'
    )
}

function Set-WinCommanderVisibility {
    <#
    .SYNOPSIS
        Toggle WinCommander visibility in Start Menu and Installed Apps (ARP) where possible.
    #>
    param(
        [bool]$Hidden = $true
    )

    Assert-IsAdmin

    $itemsChanged = 0
    $warnings = @()
    $statePath = Get-WinCommanderVisibilityStatePath
    if ([string]::IsNullOrEmpty($statePath)) {
        $warnings += "State path unavailable: ProgramData environment variable is not set. State will not be saved or restored."
    }

    try {
        if ($Hidden) {
            $state = @{
                hiddenShortcuts = @()
                uninstallKeys   = @()
                hiddenRunValues = @()
                hiddenAppPaths  = @()
            }

            # Write the flag file so a cold-start also starts hidden.
            $flagPath = Get-WinCommanderHideFlagPath
            if (-not [string]::IsNullOrEmpty($flagPath)) {
                $flagDir = Split-Path -Path $flagPath -Parent
                if (-not (Test-Path $flagDir)) { New-Item -ItemType Directory -Path $flagDir -Force | Out-Null }
                Set-Content -Path $flagPath -Value "" -Force
            }

            # ── PHASE 1: COLLECT all data before any mutations ───────────────────
            # Shortcuts are deleted in phase 2; reading them here first means
            # the state file is written before anything is removed, so an
            # interrupted run can always be restored.

            $runPaths = @(
                "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run",
                "HKCU:\Software\Microsoft\Windows\CurrentVersion\RunOnce",
                "HKLM:\Software\Microsoft\Windows\CurrentVersion\Run"
            )
            foreach ($rp in $runPaths) {
                if (-not (Test-Path $rp)) { continue }
                $props = Get-ItemProperty $rp -ErrorAction SilentlyContinue
                $props.PSObject.Properties | Where-Object {
                    $_.Name -notmatch "^PS" -and
                    $_.Name -match "WinCommander|wincommander-free|commander-free" -and
                    $_.Name -notlike "*$WinCommanderHiddenSuffix" -and
                    $_.Name -notlike "*$WinCommanderLegacyHiddenSuffix"
                } | ForEach-Object {
                    $hiddenName = "$($_.Name)$WinCommanderHiddenSuffix"
                    $state.hiddenRunValues += @{ path = $rp; original = $_.Name; hidden = $hiddenName; value = $_.Value }
                }
            }

            $wsh = New-Object -ComObject WScript.Shell
            $seenShortcutPaths = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
            foreach ($target in Get-WinCommanderStartMenuTargets) {
                try {
                    if (Test-Path $target -PathType Leaf) {
                        if (-not $seenShortcutPaths.Add($target)) { continue }
                        $entry = @{ path = $target }
                        try {
                            $sc = $wsh.CreateShortcut($target)
                            $entry.targetPath       = $sc.TargetPath
                            $entry.workingDirectory = $sc.WorkingDirectory
                            $entry.iconLocation     = $sc.IconLocation
                            $entry.arguments        = $sc.Arguments
                            $entry.description      = $sc.Description
                            $entry.windowStyle      = $sc.WindowStyle
                        } catch { $warnings += "Shortcut read ${target}: $($_.Exception.Message)" }
                        $state.hiddenShortcuts += $entry
                    }
                    elseif (Test-Path $target -PathType Container) {
                        Get-ChildItem -Path $target -Filter '*.lnk' -Recurse -ErrorAction SilentlyContinue | ForEach-Object {
                            $lnkPath = $_.FullName
                            if (-not $seenShortcutPaths.Add($lnkPath)) { return }
                            $entry = @{ path = $lnkPath }
                            try {
                                $sc = $wsh.CreateShortcut($lnkPath)
                                $entry.targetPath       = $sc.TargetPath
                                $entry.workingDirectory = $sc.WorkingDirectory
                                $entry.iconLocation     = $sc.IconLocation
                                $entry.arguments        = $sc.Arguments
                                $entry.description      = $sc.Description
                                $entry.windowStyle      = $sc.WindowStyle
                            } catch { $warnings += "Shortcut read $($_.FullName): $($_.Exception.Message)" }
                            $state.hiddenShortcuts += $entry
                        }
                    }
                } catch { $warnings += "Shortcut collect ${target}: $($_.Exception.Message)" }
            }

            foreach ($key in (Get-WinCommanderUninstallKeys)) {
                if (-not (Test-Path $key)) { continue }
                try {
                    $props = Get-ItemProperty -Path $key -ErrorAction SilentlyContinue
                    $hadSC  = $null -ne $props.PSObject.Properties['SystemComponent']
                    $state.uninstallKeys += @{
                        path = $key
                        hadSystemComponent   = $hadSC
                        systemComponentValue = if ($hadSC) { [int]$props.SystemComponent } else { $null }
                    }
                } catch { $warnings += "Uninstall key collect ${key}: $($_.Exception.Message)" }
            }

            foreach ($apPath in (Get-WinCommanderAppPaths)) {
                if (-not (Test-Path $apPath)) { continue }
                $leaf       = Split-Path $apPath -Leaf
                $hiddenPath = "$(Split-Path $apPath -Parent)\$WinCommanderHiddenPrefix$leaf"
                $state.hiddenAppPaths += @{ original = $apPath; hidden = $hiddenPath }
            }

            # ── PHASE 2: SAVE STATE — must succeed before any destructive step ──
            if (-not [string]::IsNullOrEmpty($statePath)) {
                try {
                    $stateDir = Split-Path -Path $statePath -Parent
                    if (-not (Test-Path $stateDir)) { New-Item -Path $stateDir -ItemType Directory -Force | Out-Null }
                    ($state | ConvertTo-Json -Depth 5) | Set-Content -Path $statePath -Encoding UTF8 -Force
                }
                catch {
                    # State could not be written — abort before touching any shortcuts.
                    return @{ error = $true; message = "Aborted: could not save restore state - $($_.Exception.Message). No changes were made." }
                }
            }

            # ── PHASE 3: APPLY MUTATIONS (state is already on disk) ──────────────

            # Rename Run/RunOnce values.
            foreach ($entry in $state.hiddenRunValues) {
                try {
                    if (-not (Test-Path $entry.path)) { continue }
                    New-ItemProperty $entry.path -Name $entry.hidden -Value $entry.value -PropertyType String -Force | Out-Null
                    Remove-ItemProperty $entry.path -Name $entry.original -ErrorAction SilentlyContinue
                    $itemsChanged++
                } catch { $warnings += "Run key hide $($entry.original): $($_.Exception.Message)" }
            }

            # Delete shortcuts.
            foreach ($entry in $state.hiddenShortcuts) {
                try {
                    $p = "$($entry.path)"
                    if (-not (Test-Path $p)) { continue }
                    Remove-Item -Path $p -Force -ErrorAction Stop
                    $itemsChanged++
                } catch { $warnings += "Shortcut delete $($entry.path): $($_.Exception.Message)" }
            }
            # Remove now-empty shortcut folders.
            foreach ($target in (Get-WinCommanderStartMenuTargets | Where-Object { Test-Path $_ -PathType Container })) {
                $remaining = Get-ChildItem $target -ErrorAction SilentlyContinue
                if (-not $remaining) { Remove-Item $target -Force -ErrorAction SilentlyContinue }
            }

            # Set SystemComponent=1 on uninstall keys.
            foreach ($entry in $state.uninstallKeys) {
                try {
                    if (-not (Test-Path $entry.path)) { continue }
                    New-ItemProperty -Path $entry.path -Name 'SystemComponent' -PropertyType DWord -Value 1 -Force | Out-Null
                    $itemsChanged++
                } catch { $warnings += "Uninstall key hide $($entry.path): $($_.Exception.Message)" }
            }

            # Rename App Paths keys.
            foreach ($entry in $state.hiddenAppPaths) {
                try {
                    if (-not (Test-Path $entry.original)) { continue }
                    if (Test-Path $entry.hidden) { continue }   # already renamed
                    Rename-Item -Path $entry.original -NewName ($WinCommanderHiddenPrefix + (Split-Path $entry.original -Leaf)) -Force
                    $itemsChanged++
                } catch { $warnings += "App Paths hide $($entry.original): $($_.Exception.Message)" }
            }

            # Tell Windows Shell to refresh its app list so Search clears the result immediately.
            Invoke-ShellChangeNotify

            $result = @{ status = 'hidden'; itemsChanged = $itemsChanged }
            if ($warnings.Count -gt 0) { $result.warnings = $warnings }
            return $result
        }

        # ── RESTORE (toggle off) ─────────────────────────────────────────────

        # Remove the cold-start flag so the next launch shows normally.
        $flagPath = Get-WinCommanderHideFlagPath
        if (-not [string]::IsNullOrEmpty($flagPath) -and (Test-Path $flagPath)) {
            Remove-Item $flagPath -Force -ErrorAction SilentlyContinue
        }
        $legacyFlagPath = "$env:APPDATA\WinCommander\session_state.dat"
        if (Test-Path $legacyFlagPath) { Remove-Item $legacyFlagPath -Force -ErrorAction SilentlyContinue }

        # Load saved state if available.
        $savedState = $null
        if (-not [string]::IsNullOrEmpty($statePath) -and (Test-Path $statePath)) {
            try { $savedState = Get-Content -Path $statePath -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop }
            catch { $warnings += "State load failed: $($_.Exception.Message)" }
        }

        # ── 1. SHORTCUTS ─────────────────────────────────────────────────────
        # Three strategies in order of preference:
        #   A) Recreate from full .lnk data saved in state (new format)
        #   B) Rename back *.wc-hidden files (old format / fallback)
        #   C) Warn — re-run installer

        $shortcutErrors = 0

        # Strategy A — recreate from state (field: "path" + "targetPath").
        $stateShortcuts = @()
        if ($savedState -and $savedState.hiddenShortcuts) {
            $stateShortcuts = @($savedState.hiddenShortcuts) | Where-Object { -not [string]::IsNullOrEmpty("$($_.path)") }
        }

        if ($stateShortcuts.Count -gt 0) {
            $wsh = New-Object -ComObject WScript.Shell
            $fallbackExe = $null
            foreach ($entry in $stateShortcuts) {
                try {
                    $path = "$($entry.path)"
                    if (Test-Path $path) { continue }   # already exists

                    $parentDir = Split-Path $path -Parent
                    if (-not (Test-Path $parentDir)) { New-Item -ItemType Directory -Path $parentDir -Force | Out-Null }

                    $target      = "$($entry.targetPath)"
                    $savedTarget = $target
                    $usedFallback = $false

                    if ([string]::IsNullOrEmpty($target) -or -not (Test-Path $target -PathType Leaf -ErrorAction SilentlyContinue)) {
                        if ($null -eq $fallbackExe) { $fallbackExe = Get-WinCommanderExePath }
                        if ($null -ne $fallbackExe) { $target = $fallbackExe; $usedFallback = $true }
                        elseif ([string]::IsNullOrEmpty($target)) {
                            $warnings += "Shortcut restore ${path}: exe not found - skipped."
                            $shortcutErrors++; continue
                        }
                    }

                    $iconLocation = "$($entry.iconLocation)"
                    if ($usedFallback -and -not [string]::IsNullOrEmpty($savedTarget)) {
                        $iconLocation = $iconLocation -replace [regex]::Escape($savedTarget), $target
                    }

                    $sc = $wsh.CreateShortcut($path)
                    $sc.TargetPath = $target
                    if (-not [string]::IsNullOrEmpty($entry.workingDirectory)) { $sc.WorkingDirectory = $entry.workingDirectory }
                    if (-not [string]::IsNullOrEmpty($iconLocation))           { $sc.IconLocation     = $iconLocation }
                    if (-not [string]::IsNullOrEmpty($entry.arguments))        { $sc.Arguments        = $entry.arguments }
                    if (-not [string]::IsNullOrEmpty($entry.description))      { $sc.Description      = $entry.description }
                    if ($entry.windowStyle -and [int]$entry.windowStyle -ne 0) { $sc.WindowStyle      = [int]$entry.windowStyle }
                    $sc.Save()
                    $itemsChanged++
                }
                catch { $warnings += "Shortcut restore $($entry.path): $($_.Exception.Message)"; $shortcutErrors++ }
            }
        }

        # Strategy B — rename *.wc-hidden back (handles old-format state + any leftover renamed files).
        # Always run this as a safety net regardless of whether Strategy A ran.
        foreach ($target in Get-WinCommanderStartMenuTargets) {
            try {
                # Direct .lnk hidden as .lnk.wc-hidden
                $hiddenLnk = "$target.wc-hidden"
                if ((Test-Path $hiddenLnk) -and -not (Test-Path $target)) {
                    $parentDir = Split-Path $target -Parent
                    if (-not (Test-Path $parentDir)) { New-Item -ItemType Directory -Path $parentDir -Force | Out-Null }
                    Move-Item -Path $hiddenLnk -Destination $target -Force
                    $itemsChanged++
                }
                # Folder hidden as folder.wc-hidden
                $hiddenDir = "$target.wc-hidden"
                if ((Test-Path $hiddenDir -PathType Container) -and -not (Test-Path $target)) {
                    Move-Item -Path $hiddenDir -Destination $target -Force
                    $itemsChanged++
                }
            }
            catch { $warnings += "Shortcut rename-back ${target}: $($_.Exception.Message)" }
        }

        if ($stateShortcuts.Count -eq 0 -and $shortcutErrors -eq 0) {
            # Strategy A had nothing to restore and Strategy B may have handled it via .wc-hidden files.
            # If neither found anything, warn but don't block.
            if ($itemsChanged -eq 0) {
                $warnings += "No shortcut state found - shortcuts may need to be restored by re-running the installer."
            }
        }

        # ── 2. APPS & FEATURES (uninstall keys / SystemComponent) ─────────────
        # Always scan ALL WinCommander uninstall keys and remove SystemComponent=1.
        # This is safe to run unconditionally — removing a non-existent property is a no-op.
        foreach ($key in (Get-WinCommanderUninstallKeys)) {
            try {
                # Normalise PSPath → HKLM: / HKCU: drive path so Test-Path works reliably.
                $keyPath = $key -replace '^Microsoft\.PowerShell\.Core\\Registry::', ''
                $keyPath = $keyPath -replace '^HKEY_LOCAL_MACHINE', 'HKLM:'
                $keyPath = $keyPath -replace '^HKEY_CURRENT_USER', 'HKCU:'
                if (-not (Test-Path $keyPath)) { continue }
                $props = Get-ItemProperty -Path $keyPath -ErrorAction SilentlyContinue
                if ($null -ne $props -and $null -ne $props.PSObject.Properties['SystemComponent']) {
                    # Restore to original value if we have state, otherwise just remove.
                    $savedEntry = if ($savedState -and $savedState.uninstallKeys) {
                        @($savedState.uninstallKeys) | Where-Object { "$($_.path)" -like "*$(Split-Path $keyPath -Leaf)*" } | Select-Object -First 1
                    } else { $null }

                    if ($savedEntry -and [bool]$savedEntry.hadSystemComponent -and $null -ne $savedEntry.systemComponentValue) {
                        New-ItemProperty -Path $keyPath -Name 'SystemComponent' -PropertyType DWord -Value ([int]$savedEntry.systemComponentValue) -Force | Out-Null
                    } else {
                        Remove-ItemProperty -Path $keyPath -Name 'SystemComponent' -ErrorAction SilentlyContinue
                    }
                    $itemsChanged++
                }
            }
            catch { $warnings += "Uninstall key restore ${key}: $($_.Exception.Message)" }
        }

        # ── 3. RUN KEY AUTOSTART ──────────────────────────────────────────────
        # Always scan for current and legacy hidden suffixed values and rename back.
        $runPaths = @(
            "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run",
            "HKCU:\Software\Microsoft\Windows\CurrentVersion\RunOnce",
            "HKLM:\Software\Microsoft\Windows\CurrentVersion\Run"
        )
        # Merge state entries with a live scan to catch any the state missed.
        $runEntries = [System.Collections.Generic.List[hashtable]]::new()
        if ($savedState -and $savedState.hiddenRunValues) {
            foreach ($e in @($savedState.hiddenRunValues)) { [void]$runEntries.Add(@{ path = "$($e.path)"; original = "$($e.original)"; hidden = "$($e.hidden)"; value = "$($e.value)" }) }
        }
        foreach ($rp in $runPaths) {
            if (-not (Test-Path $rp)) { continue }
            (Get-ItemProperty $rp -ErrorAction SilentlyContinue).PSObject.Properties | Where-Object {
                ($_.Name -like "*$WinCommanderHiddenSuffix" -or $_.Name -like "*$WinCommanderLegacyHiddenSuffix") -and
                $_.Name -match "WinCommander|wincommander-free|commander-free"
            } | ForEach-Object {
                $hiddenName = $_.Name
                $orig = $_.Name -replace ([regex]::Escape($WinCommanderHiddenSuffix) + '$'), ''
                $orig = $orig -replace ([regex]::Escape($WinCommanderLegacyHiddenSuffix) + '$'), ''
                if (-not ($runEntries | Where-Object { $_.hidden -eq $hiddenName -and $_.path -eq $rp })) {
                    [void]$runEntries.Add(@{ path = $rp; original = $orig; hidden = $hiddenName; value = $_.Value })
                }
            }
        }
        foreach ($entry in $runEntries) {
            try {
                $rp = $entry.path
                if (-not (Test-Path $rp)) { continue }
                if (-not [string]::IsNullOrEmpty($entry.original) -and
                    -not (Get-ItemProperty $rp -Name $entry.original -ErrorAction SilentlyContinue)) {
                    New-ItemProperty $rp -Name $entry.original -Value $entry.value -PropertyType String -Force | Out-Null
                }
                if (-not [string]::IsNullOrEmpty($entry.hidden)) {
                    Remove-ItemProperty $rp -Name $entry.hidden -ErrorAction SilentlyContinue
                }
                $itemsChanged++
            } catch { $warnings += "Run key restore $($entry.original): $($_.Exception.Message)" }
        }

        # ── 4. APP PATHS REGISTRY ─────────────────────────────────────────────
        # Always scan for current and legacy hidden-prefixed keys and rename back.
        $apRoots = @(
            'HKLM:\Software\Microsoft\Windows\CurrentVersion\App Paths',
            'HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths'
        )
        foreach ($root in $apRoots) {
            if (-not (Test-Path $root)) { continue }
            Get-ChildItem $root -ErrorAction SilentlyContinue | Where-Object {
                $_.PSChildName -like "$WinCommanderHiddenPrefix*" -or
                $_.PSChildName -like "$WinCommanderLegacyHiddenPrefix*"
            } | ForEach-Object {
                try {
                    $originalName = $_.PSChildName -replace ('^' + [regex]::Escape($WinCommanderHiddenPrefix)), ''
                    $originalName = $originalName -replace ('^' + [regex]::Escape($WinCommanderLegacyHiddenPrefix)), ''
                    $originalPath = "$root\$originalName"
                    if (-not (Test-Path $originalPath)) {
                        Rename-Item -Path $_.PSPath -NewName $originalName -Force
                        $itemsChanged++
                    }
                } catch { $warnings += "App Paths restore $($_.PSChildName): $($_.Exception.Message)" }
            }
        }

        # ── 5. CLEAN UP STATE FILE ────────────────────────────────────────────
        if ($shortcutErrors -eq 0 -and -not [string]::IsNullOrEmpty($statePath) -and (Test-Path $statePath)) {
            try { Remove-Item -Path $statePath -Force -ErrorAction SilentlyContinue } catch {}
        } elseif ($shortcutErrors -gt 0) {
            $warnings += "$shortcutErrors shortcut(s) failed - state kept for retry."
        }

        # ── 6. NOTIFY WINDOWS SHELL + SEARCH ─────────────────────────────────
        Invoke-ShellChangeNotify

        $result = @{ status = 'visible'; itemsChanged = $itemsChanged }
        if ($warnings.Count -gt 0) { $result.warnings = $warnings }
        return $result
    }
    catch {
        return @{ error = $true; message = "Failed to update WinCommander visibility: $($_.Exception.Message)" }
    }
}

function Set-WinCommanderCalculatorShortcuts {
    <#
    .SYNOPSIS
        Hides or restores WinCommander Start-Menu shortcuts for calculator mode.
        Unlike Set-WinCommanderVisibility, this does NOT write session_state.dat,
        modify Run/RunOnce keys, or touch uninstall/App-Paths registry entries.
    .PARAMETER Hidden
        $true  — delete shortcuts and save metadata to calc_shortcut_state.json.
        $false — recreate shortcuts from saved metadata and delete the state file.
    #>
    param([bool]$Hidden = $true)

    $stateDir     = Get-WinCommanderMachineStateDir
    if ([string]::IsNullOrEmpty($stateDir)) { $stateDir = "$env:APPDATA\WinCommander" }
    $statePath    = "$stateDir\calc_shortcut_state.json"
    $itemsChanged = 0
    $warnings     = @()

    try {
        if ($Hidden) {
            # ── COLLECT shortcut metadata before deleting ─────────────────────
            $wsh       = New-Object -ComObject WScript.Shell
            $shortcuts = [System.Collections.Generic.List[hashtable]]::new()
            $seen      = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)

            foreach ($target in Get-WinCommanderStartMenuTargets) {
                try {
                    if (Test-Path $target -PathType Leaf) {
                        if ($seen.Add($target)) {
                            $entry = @{ path = $target }
                            try {
                                $sc = $wsh.CreateShortcut($target)
                                $entry.targetPath       = $sc.TargetPath
                                $entry.workingDirectory = $sc.WorkingDirectory
                                $entry.iconLocation     = $sc.IconLocation
                                $entry.arguments        = $sc.Arguments
                                $entry.description      = $sc.Description
                                $entry.windowStyle      = $sc.WindowStyle
                            } catch {}
                            $shortcuts.Add($entry)
                        }
                    } elseif (Test-Path $target -PathType Container) {
                        Get-ChildItem -Path $target -Filter '*.lnk' -Recurse -ErrorAction SilentlyContinue | ForEach-Object {
                            if ($seen.Add($_.FullName)) {
                                $entry = @{ path = $_.FullName }
                                try {
                                    $sc = $wsh.CreateShortcut($_.FullName)
                                    $entry.targetPath       = $sc.TargetPath
                                    $entry.workingDirectory = $sc.WorkingDirectory
                                    $entry.iconLocation     = $sc.IconLocation
                                    $entry.arguments        = $sc.Arguments
                                    $entry.description      = $sc.Description
                                    $entry.windowStyle      = $sc.WindowStyle
                                } catch {}
                                $shortcuts.Add($entry)
                            }
                        }
                    }
                } catch { $warnings += "Collect ${target}: $($_.Exception.Message)" }
            }

            # Save state so restore knows what to recreate
            if (-not (Test-Path $stateDir)) { New-Item -ItemType Directory -Path $stateDir -Force | Out-Null }
            @{ hiddenShortcuts = @($shortcuts) } | ConvertTo-Json -Depth 5 |
                Set-Content -Path $statePath -Encoding UTF8 -Force

            # Delete shortcuts
            foreach ($entry in $shortcuts) {
                try {
                    if (Test-Path $entry.path) {
                        Remove-Item -Path $entry.path -Force -ErrorAction Stop
                        $itemsChanged++
                    }
                } catch { $warnings += "Delete $($entry.path): $($_.Exception.Message)" }
            }

            # Remove empty shortcut folders
            foreach ($target in (Get-WinCommanderStartMenuTargets | Where-Object { Test-Path $_ -PathType Container })) {
                try {
                    if (-not (Get-ChildItem $target -ErrorAction SilentlyContinue)) {
                        Remove-Item $target -Force -ErrorAction SilentlyContinue
                    }
                } catch {}
            }

            Invoke-ShellChangeNotify
            $result = @{ status = 'hidden'; itemsChanged = $itemsChanged }
            if ($warnings.Count -gt 0) { $result.warnings = $warnings }
            return $result

        } else {
            # ── RESTORE shortcuts from saved state ────────────────────────────
            if (-not (Test-Path $statePath)) {
                return @{ status = 'restored'; itemsChanged = 0 }
            }

            $savedState = $null
            try {
                $savedState = Get-Content -Path $statePath -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
            } catch {
                return @{ status = 'error'; message = "Failed to load shortcut state: $($_.Exception.Message)" }
            }

            $stateShortcuts = @()
            if ($savedState -and $savedState.hiddenShortcuts) {
                $stateShortcuts = @($savedState.hiddenShortcuts) | Where-Object { -not [string]::IsNullOrEmpty("$($_.path)") }
            }

            $shortcutErrors = 0
            if ($stateShortcuts.Count -gt 0) {
                $wsh = New-Object -ComObject WScript.Shell
                $fallbackExe = $null
                foreach ($entry in $stateShortcuts) {
                    try {
                        $path = "$($entry.path)"
                        if (Test-Path $path) { continue }

                        $parentDir = Split-Path $path -Parent
                        if (-not (Test-Path $parentDir)) { New-Item -ItemType Directory -Path $parentDir -Force | Out-Null }

                        $target       = "$($entry.targetPath)"
                        $savedTarget  = $target
                        $usedFallback = $false

                        if ([string]::IsNullOrEmpty($target) -or -not (Test-Path $target -PathType Leaf -ErrorAction SilentlyContinue)) {
                            if ($null -eq $fallbackExe) { $fallbackExe = Get-WinCommanderExePath }
                            if ($null -ne $fallbackExe) { $target = $fallbackExe; $usedFallback = $true }
                            elseif ([string]::IsNullOrEmpty($target)) {
                                $warnings += "Shortcut restore ${path}: exe not found — skipped."
                                $shortcutErrors++; continue
                            }
                        }

                        $iconLocation = "$($entry.iconLocation)"
                        if ($usedFallback -and -not [string]::IsNullOrEmpty($savedTarget)) {
                            $iconLocation = $iconLocation -replace [regex]::Escape($savedTarget), $target
                        }

                        $sc = $wsh.CreateShortcut($path)
                        $sc.TargetPath = $target
                        if (-not [string]::IsNullOrEmpty($entry.workingDirectory)) { $sc.WorkingDirectory = $entry.workingDirectory }
                        if (-not [string]::IsNullOrEmpty($iconLocation))           { $sc.IconLocation     = $iconLocation }
                        if (-not [string]::IsNullOrEmpty($entry.arguments))        { $sc.Arguments        = $entry.arguments }
                        if (-not [string]::IsNullOrEmpty($entry.description))      { $sc.Description      = $entry.description }
                        if ($entry.windowStyle -and [int]$entry.windowStyle -ne 0) { $sc.WindowStyle      = [int]$entry.windowStyle }
                        $sc.Save()
                        $itemsChanged++
                    } catch {
                        $warnings += "Shortcut restore $($entry.path): $($_.Exception.Message)"
                        $shortcutErrors++
                    }
                }
            }

            if ($shortcutErrors -eq 0) {
                try { Remove-Item -Path $statePath -Force -ErrorAction SilentlyContinue } catch {}
            } else {
                $warnings += "$shortcutErrors shortcut(s) failed — state kept for retry."
            }

            Invoke-ShellChangeNotify
            $result = @{ status = 'restored'; itemsChanged = $itemsChanged }
            if ($warnings.Count -gt 0) { $result.warnings = $warnings }
            return $result
        }
    } catch {
        return @{ error = $true; message = "Set-WinCommanderCalculatorShortcuts: $($_.Exception.Message)" }
    }
}

# ════════════════════════════════════════════════════════════════════════════
# BACKWARD COMPATIBILITY — kept so existing frontend calls still work
# ════════════════════════════════════════════════════════════════════════════

function Hide-BackendApps {
    <#
    .SYNOPSIS
        Backward-compatible wrapper. Delegates to Hide-AllBackendApps.
        Old frontend code calls this name; new code should use Hide-AllBackendApps.
    #>
    return Hide-AllBackendApps
}

function Set-BackendAppsVisibility {
    <#
    .SYNOPSIS
        Reversibly hide or restore supporting backend apps.
    #>
    param(
        [string]$Apps,
        [bool]$Hidden = $true
    )

    Assert-IsAdmin

    $warnings = @()

    $appList = @()
    if (-not [string]::IsNullOrEmpty($Apps)) {
        $appList = $Apps -split ','
    }

    # Per-app quiet mode (the rework the old no-op guard was waiting for) is now
    # in place, so this runs for real as a Free feature:
    #   - Set-TailscaleQuietMode preserves the tunnel (never kills tailscale-ipn,
    #     which was what made the original implementation unsafe);
    #   - uninstall keys are matched dynamically by DisplayName (version-agnostic);
    #   - Desktop shortcuts are permanently deleted on hide; Start Menu entries
    #     use backup-dir move so they can be restored; SystemComponent is toggled.
    $itemsChanged = 0

    # VeraCrypt
    if ($appList -contains 'encryptionEngine') {
        # Find the versioned Start Menu folder dynamically (e.g. "VeraCrypt 1.26.24" changes per update)
        $vcStartMenuFolders = @()
        $smBase = "C:\ProgramData\Microsoft\Windows\Start Menu\Programs"
        if (Test-Path $smBase) {
            Get-ChildItem -LiteralPath $smBase -Directory -ErrorAction SilentlyContinue |
                Where-Object { $_.Name -like 'VeraCrypt*' } |
                ForEach-Object { $vcStartMenuFolders += $_.FullName }
        }
        $shortcuts = $vcStartMenuFolders
        if ($Hidden) { Remove-DepDesktopShortcuts -Names @('VeraCrypt.lnk') }

        # Find uninstall key by DisplayName (version-agnostic)
        $vcUninstallKeys = @()
        foreach ($root in @(
            'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
            'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
        )) {
            if (-not (Test-Path $root)) { continue }
            Get-ChildItem -LiteralPath $root -ErrorAction SilentlyContinue | ForEach-Object {
                $dn = (Get-ItemProperty -LiteralPath $_.PSPath -Name DisplayName -ErrorAction SilentlyContinue).DisplayName
                if ($dn -and $dn -like 'VeraCrypt*') { $vcUninstallKeys += $_.PSPath }
            }
        }

        $itemsChanged += Move-ShortcutsReversible -Paths $shortcuts -Hidden $Hidden -AppKey 'veracrypt.exe' -Warnings ([ref]$warnings)
        if ($vcUninstallKeys.Count -gt 0) {
            $itemsChanged += Set-SystemComponentReversible -Keys $vcUninstallKeys -Hidden $Hidden -Warnings ([ref]$warnings)
        }
    }

    # Tailscale
    if ($appList -contains 'meshVpn') {
        if ($Hidden) { Remove-DepDesktopShortcuts -Names @('Tailscale.lnk') }
        $shortcuts = @(
            "C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Tailscale",
            "C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Tailscale.lnk",
            "C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Startup\Tailscale.lnk",
            "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Startup\Tailscale.lnk"
        )

        # Find uninstall keys dynamically by DisplayName so any Tailscale version
        # is matched regardless of GUID (hardcoded GUIDs break on updates).
        $tsUninstallKeys = @()
        foreach ($root in @(
            'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
            'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall',
            'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall'
        )) {
            if (-not (Test-Path $root)) { continue }
            Get-ChildItem -LiteralPath $root -ErrorAction SilentlyContinue | ForEach-Object {
                $dn = (Get-ItemProperty -LiteralPath $_.PSPath -Name DisplayName -ErrorAction SilentlyContinue).DisplayName
                if ($dn -and $dn -like 'Tailscale*') {
                    $tsUninstallKeys += $_.PSPath
                }
            }
        }

        $itemsChanged += Move-ShortcutsReversible -Paths $shortcuts -Hidden $Hidden -AppKey 'tailscale.exe' -Warnings ([ref]$warnings)
        if ($tsUninstallKeys.Count -gt 0) {
            $itemsChanged += Set-SystemComponentReversible -Keys $tsUninstallKeys -Hidden $Hidden -Warnings ([ref]$warnings)
        }

        $itemsChanged += Set-TailscaleQuietMode -Hidden $Hidden -Warnings ([ref]$warnings)
    }

    # ActivityWatch
    if ($appList -contains 'productivityEngine') {
        if ($Hidden) { Remove-DepDesktopShortcuts -Names @('ActivityWatch.lnk') }
        $shortcuts = @(
            "C:\ProgramData\Microsoft\Windows\Start Menu\Programs\ActivityWatch.lnk",
            "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Startup\ActivityWatch.lnk"
        )
        $uninstallKeys = @(
            'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{A18A23E6-901D-0CE8-035423E4}_is1'
        )
        $itemsChanged += Move-ShortcutsReversible -Paths $shortcuts -Hidden $Hidden -AppKey 'aw-qt.exe' -Warnings ([ref]$warnings)
        $itemsChanged += Set-SystemComponentReversible -Keys $uninstallKeys -Hidden $Hidden -Warnings ([ref]$warnings)

        try {
            if ($Hidden) {
                Stop-Process -Name "aw-qt" -Force -ErrorAction SilentlyContinue
                if (Get-Command "Set-WCSetting" -EA SilentlyContinue) {
                    Set-WCSetting -Path "productivity.productivityEngineStealthEnabled" -Value $true
                }
                if (Get-Command "Invoke-ProductivityEngineMaintenance" -EA SilentlyContinue) {
                    Invoke-ProductivityEngineMaintenance | Out-Null
                }
            } else {
                if (Get-Command "Stop-ProductivityTracker" -EA SilentlyContinue) {
                    Stop-ProductivityTracker | Out-Null
                } else {
                    Stop-Process -Name "aw-watcher-window", "aw-watcher-afk", "aw-server" -Force -ErrorAction SilentlyContinue
                }
                $awQtPaths = @(
                    "$env:LOCALAPPDATA\Programs\ActivityWatch\aw-qt.exe",
                    "$env:ProgramFiles\ActivityWatch\aw-qt.exe",
                    "$env:LOCALAPPDATA\Programs\ActivityWatch\aw-qt\aw-qt.exe"
                )
                foreach ($p in $awQtPaths) {
                    if (Test-Path $p) {
                        Start-Process -FilePath $p -WorkingDirectory (Split-Path $p -Parent) -ErrorAction SilentlyContinue
                        break
                    }
                }
                if (Get-Command "Set-WCSetting" -EA SilentlyContinue) {
                    Set-WCSetting -Path "productivity.productivityEngineStealthEnabled" -Value $false
                }
            }
        } catch { }
    }

    # Everything Search
    if ($appList -contains 'instantSearch') {
        if ($Hidden) { Remove-DepDesktopShortcuts -Names @('Everything.lnk') }
        $shortcuts = @(
            "C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Everything.lnk",
            "C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Everything",
            "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Everything.lnk",
            "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Everything"
        )
        $itemsChanged += Move-ShortcutsReversible -Paths $shortcuts -Hidden $Hidden -AppKey 'everything.exe' -Warnings ([ref]$warnings)
        $itemsChanged += Set-EverythingQuietMode -Hidden $Hidden -Warnings ([ref]$warnings)
    }

    # BleachBit
    if ($appList -contains 'systemCleaner') {
        if ($Hidden) { Remove-DepDesktopShortcuts -Names @('BleachBit.lnk', 'BleachBit as Administrator.lnk') }
        $shortcuts = @(
            "C:\ProgramData\Microsoft\Windows\Start Menu\Programs\BleachBit"
        )
        $itemsChanged += Move-ShortcutsReversible -Paths $shortcuts -Hidden $Hidden -AppKey 'bleachbit.exe' -Warnings ([ref]$warnings)
    }

    # UniGetUI
    if ($appList -contains 'unigetui') {
        if ($Hidden) { Remove-DepDesktopShortcuts -Names @('UniGetUI.lnk', 'WingetUI.lnk') }
        $shortcuts = @(
            "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\UniGetUI"
        )
        $uninstallKeys = @(
            'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\UniGetUI (Current user)'
        )
        $itemsChanged += Move-ShortcutsReversible -Paths $shortcuts -Hidden $Hidden -AppKey 'unigetui.exe' -Warnings ([ref]$warnings)
        $itemsChanged += Set-SystemComponentReversible -Keys $uninstallKeys -Hidden $Hidden -Warnings ([ref]$warnings)
    }

    if ($appList -contains 'ramDiskEngine') {
        # Shortcuts — ImDisk installs under several possible folder names.
        $imDiskStartMenuRoots = @(
            'C:\ProgramData\Microsoft\Windows\Start Menu\Programs\ImDisk Virtual Disk Driver',
            'C:\ProgramData\Microsoft\Windows\Start Menu\Programs\ImDisk',
            "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\ImDisk Virtual Disk Driver",
            "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\ImDisk"
        )
        if ($Hidden) { Remove-DepDesktopShortcuts -Names @('ImDisk Virtual Disk Driver.lnk', 'ImDisk.lnk') }
        $shortcuts = $imDiskStartMenuRoots

        # Uninstall keys — search dynamically by DisplayName so version changes don't break it.
        $imDiskUninstallKeys = [System.Collections.Generic.List[string]]::new()
        $knownKeys = @(
            'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\ImDisk',
            'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\ImDisk'
        )
        foreach ($k in $knownKeys) { if (Test-Path $k) { [void]$imDiskUninstallKeys.Add($k) } }
        foreach ($root in @(
            'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
            'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall',
            'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall'
        )) {
            if (-not (Test-Path $root)) { continue }
            Get-ChildItem $root -ErrorAction SilentlyContinue | ForEach-Object {
                try {
                    $dn = (Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue).DisplayName
                    if ($dn -match '^ImDisk') { [void]$imDiskUninstallKeys.Add($_.PSPath) }
                } catch {}
            }
        }

        $itemsChanged += Move-ShortcutsReversible -Paths $shortcuts -Hidden $Hidden -AppKey 'imdisk.exe' -Warnings ([ref]$warnings)
        $itemsChanged += Set-SystemComponentReversible -Keys ($imDiskUninstallKeys | Select-Object -Unique) -Hidden $Hidden -Warnings ([ref]$warnings)
        # Note: the ImDisk kernel driver (imdisk.sys) and service are intentionally left running
        # so existing RAM disks remain mounted. Only the UI shortcuts and ARP entry are hidden.
    }

    $result = @{
        status = if ($Hidden) { 'hidden' } else { 'visible' }
        itemsChanged = $itemsChanged
    }
    if ($warnings.Count -gt 0) { $result['warnings'] = $warnings }
    return $result
}

function Get-TailscaleCliPath {
    $candidates = @(
        (Get-Command "tailscale.exe" -ErrorAction SilentlyContinue).Source,
        "C:\Program Files\Tailscale\tailscale.exe",
        "C:\Program Files (x86)\Tailscale\tailscale.exe",
        "$env:LOCALAPPDATA\Programs\Tailscale\tailscale.exe"
    ) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }

    foreach ($path in $candidates) {
        if (Test-Path $path -ErrorAction SilentlyContinue) { return $path }
    }
    return $null
}

function Set-TailscaleQuietMode {
    param(
        [bool]$Hidden,
        $Warnings
    )

    $changed = 0
    $cli = Get-TailscaleCliPath
    if (-not $cli) {
        $Warnings.Value += "Tailscale CLI not found; tray process was left unchanged."
        return $changed
    }

    if ($Hidden) {
        try {
            Set-Service -Name "Tailscale" -StartupType Automatic -ErrorAction SilentlyContinue
            Start-Service -Name "Tailscale" -ErrorAction SilentlyContinue

            & $cli ip --4 2>$null | Out-Null
            if ($LASTEXITCODE -ne 0) {
                $Warnings.Value += "Tailscale is not connected; tray process was left unchanged."
                return $changed
            }

            # Enable unattended mode so the VPN tunnel continues running in the background
            # when the tray process (tailscale-ipn.exe) is stopped.
            & $cli set --unattended=true 2>$null | Out-Null

            # Stop tailscale-ipn.exe tray application to hide it from the system tray,
            # but ensure the Tailscale background service remains running so it still works.
            $isRunning = Get-Process -Name 'tailscale-ipn' -ErrorAction SilentlyContinue
            if ($isRunning) {
                Stop-Process -Name 'tailscale-ipn' -Force -ErrorAction SilentlyContinue
                $changed++
            }
        }
        catch {
            $Warnings.Value += "Tailscale quiet mode: $($_.Exception.Message)"
        }
    }
    else {
        try {
            $isRunning = Get-Process -Name 'tailscale-ipn' -ErrorAction SilentlyContinue
            if (-not $isRunning) {
                $tsPaths = @(
                    "C:\Program Files\Tailscale\tailscale-ipn.exe",
                    "C:\Program Files (x86)\Tailscale\tailscale-ipn.exe",
                    "$env:LOCALAPPDATA\Programs\Tailscale\tailscale-ipn.exe"
                )
                foreach ($p in $tsPaths) {
                    if (Test-Path $p) {
                        Start-Process -FilePath $p -WindowStyle Hidden -ErrorAction SilentlyContinue
                        $changed++
                        break
                    }
                }
            }
        }
        catch {
            $Warnings.Value += "Tailscale restore: $($_.Exception.Message)"
        }
    }

    return $changed
}

function Get-EverythingIniPaths {
    $paths = New-Object System.Collections.Generic.List[string]
    foreach ($candidate in @(
        "$env:APPDATA\Everything\Everything.ini",
        "$env:LOCALAPPDATA\Everything\Everything.ini",
        "$env:ProgramFiles\Everything\Everything.ini",
        "${env:ProgramFiles(x86)}\Everything\Everything.ini"
    )) {
        if (-not [string]::IsNullOrWhiteSpace($candidate)) {
            [void]$paths.Add($candidate)
        }
    }
    return $paths | Select-Object -Unique
}

function Set-IniValue {
    param(
        [string]$Path,
        [string]$Name,
        [string]$Value
    )

    $lines = @()
    if (Test-Path $Path) {
        $lines = @(Get-Content -LiteralPath $Path -ErrorAction SilentlyContinue)
    }

    $found = $false
    $updated = @()
    foreach ($line in $lines) {
        if ($line -match "^\s*$([regex]::Escape($Name))\s*=") {
            $updated += "$Name=$Value"
            $found = $true
        }
        else {
            $updated += $line
        }
    }
    if (-not $found) { $updated += "$Name=$Value" }

    $dir = Split-Path $Path -Parent
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
    Set-Content -LiteralPath $Path -Value $updated -Encoding UTF8
}

function Get-EverythingExePath {
    $candidates = @(
        (Get-Command "Everything.exe" -ErrorAction SilentlyContinue).Source,
        "$env:ProgramFiles\Everything\Everything.exe",
        "${env:ProgramFiles(x86)}\Everything\Everything.exe",
        "$env:ProgramFiles\WinGet\Links\Everything.exe",
        "$env:ProgramData\Microsoft\WinGet\Links\Everything.exe",
        "$env:LOCALAPPDATA\Programs\Everything\Everything.exe",
        "$env:LOCALAPPDATA\Everything\Everything.exe"
    ) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }

    foreach ($path in $candidates) {
        if (Test-Path $path -ErrorAction SilentlyContinue) { return $path }
    }
    return $null
}

function Set-EverythingQuietMode {
    param(
        [bool]$Hidden,
        $Warnings
    )

    $changed = 0
    $iniPaths = @(Get-EverythingIniPaths | Where-Object { Test-Path $_ })
    if ($iniPaths.Count -eq 0) {
        $defaultIni = "$env:APPDATA\Everything\Everything.ini"
        if (-not [string]::IsNullOrWhiteSpace($defaultIni)) { $iniPaths = @($defaultIni) }
    }

    foreach ($ini in $iniPaths) {
        try {
            $trayValue = if ($Hidden) { "0" } else { "1" }
            Set-IniValue -Path $ini -Name "show_tray_icon" -Value $trayValue
            Set-IniValue -Path $ini -Name "run_in_background" -Value "1"
            $changed++
        }
        catch {
            $Warnings.Value += "Everything.ini update failed at ${ini}: $($_.Exception.Message)"
        }
    }

    try {
        Stop-Process -Name "Everything" -Force -ErrorAction SilentlyContinue
        $exe = Get-EverythingExePath
        if ($exe) {
            Start-Process -FilePath $exe -WorkingDirectory (Split-Path $exe -Parent) -WindowStyle Hidden -ErrorAction SilentlyContinue
            $changed++
        }
        else {
            $Warnings.Value += "Everything.exe not found; settings will apply next time Everything starts."
        }
    }
    catch {
        $Warnings.Value += "Everything restart failed: $($_.Exception.Message)"
    }

    return $changed
}

function Remove-DepDesktopShortcuts {
    param([string[]]$Names)
    $desktops = @(
        [Environment]::GetFolderPath('Desktop'),
        [Environment]::GetFolderPath('CommonDesktopDirectory')
    ) | Where-Object { -not [string]::IsNullOrEmpty($_) } | Select-Object -Unique
    foreach ($name in $Names) {
        foreach ($d in $desktops) {
            $p = Join-Path $d $name
            Remove-Item $p -Force -ErrorAction SilentlyContinue
        }
    }
}

function Move-ShortcutsReversible {
    param(
        [string[]]$Paths,
        [bool]$Hidden,
        [string]$AppKey,
        $Warnings
    )
    $backupDir = "$env:APPDATA\WinCommander\runtime_visibility\shortcuts\$AppKey"
    $changed = 0
    foreach ($p in $Paths) {
        try {
            $fileName   = Split-Path $p -Leaf
            $backupPath = "$backupDir\$fileName"

            if ($Hidden) {
                if (Test-Path $p) {
                    if (-not (Test-Path $backupPath)) {
                        if (-not (Test-Path $backupDir)) {
                            New-Item -ItemType Directory -Path $backupDir -Force | Out-Null
                        }
                        Move-Item -Path $p -Destination $backupPath -Force
                        $changed++
                    }
                }
            } else {
                if (Test-Path $backupPath) {
                    $parentDir = Split-Path $p -Parent
                    if (-not (Test-Path $parentDir)) {
                        New-Item -ItemType Directory -Path $parentDir -Force | Out-Null
                    }
                    if (-not (Test-Path $p)) {
                        Move-Item -Path $backupPath -Destination $p -Force
                        $changed++
                    }
                }
            }
        }
        catch {
            $Warnings.Value += "Shortcut error on ${p}: $($_.Exception.Message)"
        }
    }
    return $changed
}

function Set-SystemComponentReversible {
    param(
        [string[]]$Keys,
        [bool]$Hidden,
        $Warnings
    )
    $changed = 0
    foreach ($key in $Keys) {
        try {
            if (Test-Path $key) {
                if ($Hidden) {
                    New-ItemProperty -Path $key -Name 'SystemComponent' -PropertyType DWord -Value 1 -Force | Out-Null
                } else {
                    Remove-ItemProperty -Path $key -Name 'SystemComponent' -ErrorAction SilentlyContinue
                }
                $changed++
            }
        }
        catch {
            $Warnings.Value += "Registry error on ${key}: $($_.Exception.Message)"
        }
    }
    return $changed
}
