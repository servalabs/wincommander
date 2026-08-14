# ============================================================================
# APPS - WINGET MODULE
# Manages software installation and updates using Windows Package Manager
# ============================================================================

# Helper: Get standard apps manifest
function Read-AppsManifest {
    $apps = @(
        @{ category = "base"; id = "Nilesoft.Shell"; name = "Custom Context Menu"; description = "Powerful shell extension and context menu customizer." },
        @{ category = "base"; id = "Giorgiotani.Peazip"; name = "PeaZip"; description = "Lightweight archive manager supporting many formats." },
        @{ category = "base"; id = "Voidtools.Everything"; name = "Search Engine"; description = "Instant full-disk file search engine. WinCommander also installs its required search CLI." },
        @{ category = "base"; id = "DuongDieuPhap.ImageGlass"; name = "ImageGlass"; description = "Fast, modern image viewer." },
        @{ category = "base"; id = "Starpine.Screenbox"; name = "Screenbox"; description = "Modern media player for Windows." },
        @{ category = "base"; id = "VideoLAN.VLC"; name = "VLC Media Player"; description = "Free, open-source media player that plays almost any format." },
        @{ category = "base"; id = "IObit.DriverBooster"; name = "Driver Updater"; description = "Automatic driver update utility to keep your system drivers up-to-date." },

        @{ category = "dev"; id = "GitHub.GitHubDesktop"; name = "GitHub Desktop"; description = "Git desktop client for managing repositories." },
        @{ category = "dev"; id = "PostgreSQL.pgAdmin"; name = "pgAdmin"; description = "Administration and development platform for PostgreSQL." },
        @{ category = "dev"; id = "Anysphere.Cursor"; name = "Cursor"; description = "AI pair-programming IDE based on VS Code." },
        @{ category = "dev"; id = "Google.Antigravity"; name = "Antigravity"; description = "Agentic development environment from Google." },
        @{ category = "dev"; id = "Microsoft.WindowsTerminal"; name = "Windows Terminal"; description = "Modern terminal with tabs and profiles." },
        @{ category = "dev"; id = "OpenJS.NodeJS"; name = "Node.js"; description = "JavaScript runtime for server-side and tooling." },
        @{ category = "dev"; id = "Oven-sh.Bun"; name = "Bun"; description = "Fast JavaScript runtime, package manager, bundler, and test runner." },
        @{ category = "dev"; id = "Cloudflare.cloudflared"; name = "cloudflared"; description = "Cloudflare Tunnel command-line client." },
        @{ category = "dev"; id = "Microsoft.PowerShell"; name = "PowerShell 7"; description = "Cross-platform automation and configuration shell." },
        @{ category = "dev"; id = "Git.Git"; name = "Git"; description = "Distributed version control system." },
        @{ category = "dev"; id = "Oracle.JDK.25"; name = "Oracle JDK 25"; description = "Java Development Kit (LTS)." },
        @{ category = "dev"; id = "Python.Python.3.12"; name = "Python 3.12"; description = "Python programming language runtime 3.12." },
        @{ category = "dev"; id = "Python.Launcher"; name = "Python Launcher"; description = "Launch multiple Python versions conveniently." },
        @{ category = "dev"; id = "Mobatek.MobaXterm"; name = "MobaXterm"; description = "Enhanced terminal for Windows with X11 and SSH." },
        @{ category = "dev"; id = "Gyan.FFmpeg"; name = "FFmpeg"; description = "Cross-platform solution to record, convert and stream audio and video." },

        @{ category = "mid"; id = "PDFgear.PDFgear"; name = "PDFgear"; description = "PDF reader and editor." },
        @{ category = "mid"; id = "AntibodySoftware.WizTree"; name = "WizTree"; description = "Disk space analyzer that reads NTFS MFT." },
        @{ category = "mid"; id = "CodeSector.TeraCopy"; name = "TeraCopy"; description = "Fast and reliable file copy utility." },
        @{ category = "mid"; id = "Vivaldi.Vivaldi"; name = "Vivaldi"; description = "Customizable Chromium-based web browser." },
        @{ category = "mid"; id = "SoftDeluxe.FreeDownloadManager"; name = "Free Download Manager"; description = "Powerful modern download manager." },
        @{ category = "mid"; id = "Klocman.BulkCrapUninstaller"; name = "Bulk Uninstaller"; description = "Advanced uninstaller for batch removal and cleanup." },
        @{ category = "mid"; id = "flux.flux"; name = "f.lux"; description = "Screen color temperature adjustment." },

        @{ category = "bench-mon"; id = "REALiX.HWiNFO"; name = "HWiNFO"; description = "Comprehensive hardware analysis and monitoring." },
        @{ category = "bench-mon"; id = "CrystalDewWorld.CrystalDiskInfo"; name = "CrystalDiskInfo"; description = "SMART monitoring for HDD/SSD health." },
        @{ category = "bench-mon"; id = "CrystalDewWorld.CrystalDiskMark"; name = "CrystalDiskMark"; description = "Disk benchmark tool." },
        @{ category = "bench-mon"; id = "smartmontools.smartmontools"; name = "Disk Health Engine"; description = "S.M.A.R.T. monitoring CLI (smartctl) for deep HDD/SSD health diagnostics." },
        @{ category = "bench-mon"; id = "WinsiderSS.SystemInformer"; name = "System Informer"; description = "Advanced process viewer and system monitor." },
        @{ category = "bench-mon"; id = "Resplendence.WhoCrashed"; name = "WhoCrashed"; description = "Crash dump analyzer for diagnosing system crashes." },
        @{ category = "bench-mon"; id = "Famatech.AdvancedIPScanner"; name = "Advanced IP Scanner"; description = "Fast and free network scanner." },
        
        @{ category = "depend"; id = "Microsoft.VCRedist.2015+.x64"; name = "Visual C++ 2015-2022 (x64)"; description = "VC++ runtime packages for modern apps." },
        @{ category = "depend"; id = "Microsoft.DotNet.DesktopRuntime.6"; name = ".NET Desktop Runtime 6"; description = "Run .NET 6 desktop apps." },
        @{ category = "depend"; id = "Microsoft.DotNet.DesktopRuntime.8"; name = ".NET Desktop Runtime 8"; description = "Run .NET 8 desktop apps." },
        @{ category = "depend"; id = "Microsoft.DotNet.DesktopRuntime.10"; name = ".NET Desktop Runtime 10"; description = "Run .NET 10 desktop apps." },
        @{ category = "depend"; id = "Microsoft.DotNet.Runtime.7"; name = ".NET Runtime 7"; description = "Run .NET 7 console/server apps." },
        @{ category = "depend"; id = "Microsoft.DotNet.Runtime.8"; name = ".NET Runtime 8"; description = "Run .NET 8 console/server apps." },
        @{ category = "depend"; id = "Microsoft.DotNet.Runtime.9"; name = ".NET Runtime 9"; description = "Run .NET 9 console/server apps." },
        @{ category = "depend"; id = "Microsoft.DotNet.Runtime.10"; name = ".NET Runtime 10"; description = "Run .NET 10 console/server apps." },
        @{ category = "depend"; id = "ImDisk.Toolkit"; name = "RAM Disk Engine"; description = "RAM disk utility for creating virtual memory-backed drives." },
        
        @{ category = "misc"; id = "Google.QuickShare"; name = "Google Quick Share"; description = "Share files between devices." },
        @{ category = "misc"; id = "LocalSend.LocalSend"; name = "LocalSend"; description = "Offline, local-network file transfer." },
        @{ category = "misc"; id = "Canva.Affinity"; name = "Affinity"; description = "Creative design, photo, and publishing suite." },
        @{ category = "misc"; id = "Obsidian.Obsidian"; name = "Obsidian"; description = "Knowledge base and Markdown note-taking." },
        @{ category = "misc"; id = "Nlitesoft.NTLite"; name = "NTLite"; description = "Windows image customization tool." },
        @{ category = "misc"; id = "BillStewart.SyncthingWindowsSetup"; name = "Syncthing"; description = "Continuous file synchronization." },
        @{ category = "misc"; id = "Balena.Etcher"; name = "balenaEtcher"; description = "Flash OS images to SD cards and USB drives." },
        @{ category = "misc"; id = "BlastApps.FluentSearch"; name = "Fluent Search"; description = "Powerful search app for Windows." },
        @{ category = "misc"; id = "Ablaze.Floorp"; name = "Floorp"; description = "Firefox-based browser with customizations." },
        @{ category = "misc"; id = "AnyDesk.AnyDesk"; name = "AnyDesk"; description = "Remote desktop and support application." },
        @{ category = "misc"; id = "DucFabulous.UltraViewer"; name = "UltraViewer"; description = "Remote support and desktop-sharing application." },
        @{ category = "misc"; id = "TeamViewer.TeamViewer"; name = "TeamViewer"; description = "Remote access, support, and desktop-sharing application." },

        @{ category = "power"; id = "Microsoft.PowerToys"; name = "PowerToys"; description = "Utilities to power up Windows productivity." },
        @{ category = "power"; id = "ActivityWatch.ActivityWatch"; name = "ActivityWatch"; description = "Sovereign time tracker for local productivity analytics." },
        @{ category = "power"; id = "ShareX.ShareX"; name = "ShareX"; description = "Screen capture and productivity tool." },
        @{ category = "power"; id = "Espanso.Espanso"; name = "Espanso"; description = "Text expander for productivity." },
        @{ category = "power"; id = "AutoHotkey.AutoHotkey"; name = "AutoHotkey"; description = "Automation scripting for Windows." },
        @{ category = "power"; id = "QL-Win.QuickLook"; name = "QuickLook"; description = "Quick file preview like macOS." },
        @{ category = "power"; id = "hluk.CopyQ"; name = "CopyQ"; description = "Advanced clipboard manager." },
        @{ category = "power"; id = "Zaarrg.StremioCommunity"; name = "Stremio"; description = "Media center for streaming." },
        @{ category = "power"; id = "Winaero.Tweaker"; name = "Winaero Tweaker"; description = "Advanced Windows customization and tweaking tool." },
        @{ category = "power"; id = "Anthropic.Claude"; name = "Claude"; description = "Anthropic's AI assistant desktop app." },

        @{ category = "privacy"; id = "Proton.ProtonDrive"; name = "Proton Drive"; description = "Secure encrypted cloud storage." },
        @{ category = "privacy"; id = "IDRIX.VeraCrypt"; name = "Disk Encryption Engine"; description = "Disk encryption utility." },
        @{ category = "privacy"; id = "Proton.ProtonPass"; name = "Proton Pass"; description = "Secure password manager." },
        @{ category = "privacy"; id = "Proton.ProtonMail"; name = "Proton Mail"; description = "End-to-end encrypted email." },
        @{ category = "privacy"; id = "Proton.ProtonVPN"; name = "Proton VPN"; description = "Secure VPN service." },
        @{ category = "privacy"; id = "Tailscale.Tailscale"; name = "Private Mesh VPN"; description = "Zero-config secure mesh VPN." },
        @{ category = "privacy"; id = "OpenWhisperSystems.Signal"; name = "Signal"; description = "Private messenger for secure communication." },
        @{ category = "privacy"; id = "Brave.Brave"; name = "Brave Browser"; description = "Fast, privacy-focused browser with built-in ad blocker." },
        @{ category = "privacy"; id = "LibreWolf.LibreWolf"; name = "LibreWolf"; description = "A standalone browser that provides protection against tracking and fingerprinting." },
        @{ category = "privacy"; id = "Cryptomator.Cryptomator"; name = "Cryptomator"; description = "Client-side encryption for cloud files." },
        @{ category = "privacy"; id = "Ferdium.Ferdium"; name = "Ferdium"; description = "All-in-one app for your favorite messaging services." },
        @{ category = "privacy"; id = "BleachBit.BleachBit"; name = "BleachBit"; description = "Open source system cleaner and privacy tool." }
    )
    return $apps
}

function Get-AppManifest {
    param([string]$Category)

    $apps = Read-AppsManifest
    if (-not [string]::IsNullOrWhiteSpace($Category)) {
        $apps = @($apps | Where-Object { $_.category -eq $Category })
    }

    return @{ apps = @($apps) }
}

# Resolve Winget executable path
function Resolve-WingetPath {
    $cmd = @(Get-Command winget -ErrorAction SilentlyContinue) | Select-Object -First 1
    if ($cmd) {
        if ($cmd -is [System.Management.Automation.CommandInfo]) {
            return $cmd
        }

        if ($cmd.PSObject.Properties.Match('Source').Count -gt 0 -and -not [string]::IsNullOrWhiteSpace($cmd.Source)) {
            return [string]$cmd.Source
        }

        if ($cmd.PSObject.Properties.Match('Path').Count -gt 0 -and -not [string]::IsNullOrWhiteSpace($cmd.Path)) {
            return [string]$cmd.Path
        }

        return [string]$cmd
    }

    $candidates = @(
        "$env:LOCALAPPDATA\Microsoft\WindowsApps\winget.exe",
        "$env:LOCALAPPDATA\Microsoft\WinGet\Links\winget.exe"
    )
    foreach ($path in $candidates) {
        if (Test-Path $path) { return $path }
    }
    return $null
}

# Parse winget table output
function Parse-WingetTable {
    param(
        [string]$Raw,
        [string]$OnlySource = $null
    )

    if ([string]::IsNullOrWhiteSpace($Raw)) { return @() }

    $lines = $Raw -split "`r?`n"
    
    $items = @()
    foreach ($line in $lines) {
        $line = $line.TrimEnd()
        if ([string]::IsNullOrWhiteSpace($line)) { continue }
        if ($line -match '^Name\s+Id\s+Version') { continue }
        if ($line -match '^-{3,}') { continue }
        if ($line -match '^\d+\s+(upgrades?|packages?)') { continue }
        if ($line -match 'aka\.ms|More help') { continue }
        if ($line -match '^Windows Package Manager|^Copyright') { continue }

        # Extract ID: Must be format like Publisher.Package or ARP\...
        # IDs are like: Microsoft.PowerShell, Vivaldi.Vivaldi, ARP\Machine\X86\Vivaldi
        
        # First try to find a standard winget ID (Publisher.Package)
        $allMatches = [regex]::Matches($line, '(?i)\b([a-z0-9][a-z0-9-_\.]*\.[a-z0-9][a-z0-9-_\.\+]*)\b')
        
        $id = $null
        $bestScore = -1
        foreach ($m in $allMatches) {
            $candidate = $m.Value
            # Skip version-like patterns (e.g., "9.7.2")
            if ($candidate -match '^\d+\.\d+') { continue }
            $score = 0
            # Prefer longer IDs
            $score += $candidate.Length
            # Prefer IDs with CamelCase (Publisher.Package pattern)
            if ($candidate -match '\.[A-Z]') { $score += 50 }
            # Prefer IDs appearing later in the line (actual ID column, not name)
            $score += $m.Index / 10
            
            if ($score -gt $bestScore) {
                $bestScore = $score
                $id = $candidate
            }
        }
        
        # If no standard ID found, try ARP pattern
        if (-not $id -and $line -match '(ARP\\[^\s]+)') {
            $id = $matches[1]
        }
        
        if (-not $id) { continue }
        
        # Get the name (everything before the ID)
        $idIndex = $line.IndexOf($id)
        $name = if ($idIndex -gt 0) { $line.Substring(0, $idIndex).Trim() } else { "" }
        
        # Get everything after the ID
        $afterId = $line.Substring($idIndex + $id.Length).Trim()
        $afterParts = $afterId -split '\s+'
        
        $version = $null
        $available = $null
        $source = $null
        
        # Parse remaining parts: Version [Available] Source
        if ($afterParts.Count -ge 1) { $version = $afterParts[0] }
        if ($afterParts.Count -ge 3) { 
            $available = $afterParts[1]
            $source = $afterParts[2]
        }
        elseif ($afterParts.Count -eq 2) {
            if ($afterParts[1] -match '^(winget|msstore|store)$') {
                $source = $afterParts[1]
            }
            else {
                $available = $afterParts[1]
            }
        }

        $items += [pscustomobject]@{
            Name      = $name
            Id        = $id
            Version   = $version
            Available = $available
            Source    = $source
        }
    }

    # Filter by source after parsing if requested
    if ($OnlySource) {
        $items = $items | Where-Object { $_.Source -eq $OnlySource }
    }

    return $items
}

function Get-WinCommanderUserDataDir {
    $base = if ($env:LOCALAPPDATA) { $env:LOCALAPPDATA } else { [System.IO.Path]::GetTempPath() }
    $dir = Join-Path $base "WinCommander"
    New-Item -ItemType Directory -Force -Path $dir -ErrorAction SilentlyContinue | Out-Null
    return $dir
}

function ConvertTo-IconCacheKey {
    param([string]$Id, [string]$Name)
    $raw = "unknown"
    if (-not [string]::IsNullOrWhiteSpace($Id)) {
        $raw = $Id
    }
    elseif (-not [string]::IsNullOrWhiteSpace($Name)) {
        $raw = $Name
    }
    return ($raw -replace '[^a-zA-Z0-9._-]', '_')
}

function Normalize-DisplayIconPath {
    param([string]$Value)
    if ([string]::IsNullOrWhiteSpace($Value)) { return $null }
    $v = [Environment]::ExpandEnvironmentVariables($Value.Trim())
    $quote = [char]34
    if ($v.StartsWith($quote)) {
        $end = $v.IndexOf($quote, 1)
        if ($end -gt 1) { $v = $v.Substring(1, $end - 1) }
    }
    else {
        $comma = $v.IndexOf(',')
        if ($comma -gt 0) { $v = $v.Substring(0, $comma) }
        $exe = $v.IndexOf('.exe', [StringComparison]::OrdinalIgnoreCase)
        if ($exe -gt 0) { $v = $v.Substring(0, $exe + 4) }
    }
    $v = $v.Trim($quote, [char]32)
    if ($v -and (Test-Path -LiteralPath $v -PathType Leaf -ErrorAction SilentlyContinue)) { return $v }
    return $null
}

function Get-ShortcutTargetPath {
    param([string]$Path)
    try {
        $shell = New-Object -ComObject WScript.Shell
        $shortcut = $shell.CreateShortcut($Path)
        $target = $shortcut.TargetPath
        if ($target -and (Test-Path -LiteralPath $target -PathType Leaf -ErrorAction SilentlyContinue)) { return $target }
    }
    catch { }
    return $null
}

function Resolve-AppIconSourcePath {
    param([string]$Id, [string]$Name)

    $needleParts = @($Id, $Name) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
    # Also add the publisher-stripped tail of the winget ID (e.g. "ExifTool" from
    # "OliverBetz.ExifTool") so registry DisplayNames like "ExifTool version 13.58_64"
    # still match even when the full ID doesn't appear in the DisplayName.
    if ($Id -and $Id.Contains('.')) {
        $idTail = $Id.Split('.')[-1]
        if ($idTail -and ($needleParts -notcontains $idTail)) { $needleParts += $idTail }
    }
    $registryRoots = @(
        'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
        'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall',
        'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall'
    )

    foreach ($root in $registryRoots) {
        $keys = Get-ChildItem -LiteralPath $root -ErrorAction SilentlyContinue
        foreach ($key in $keys) {
            $p = Get-ItemProperty -LiteralPath $key.PSPath -ErrorAction SilentlyContinue
            if (-not $p) { continue }
            $displayName = [string]$p.DisplayName
            $matched = $false
            foreach ($needle in $needleParts) {
                if ($displayName -and $displayName.IndexOf($needle, [StringComparison]::OrdinalIgnoreCase) -ge 0) { $matched = $true; break }
                if ($key.PSChildName -and $key.PSChildName.IndexOf($needle, [StringComparison]::OrdinalIgnoreCase) -ge 0) { $matched = $true; break }
            }
            if (-not $matched) { continue }

            $iconPath = Normalize-DisplayIconPath $p.DisplayIcon
            if ($iconPath) { return $iconPath }

            $installLocation = [string]$p.InstallLocation
            if ($installLocation -and (Test-Path -LiteralPath $installLocation -PathType Container -ErrorAction SilentlyContinue)) {
                $exe = Get-ChildItem -LiteralPath $installLocation -Filter '*.exe' -File -ErrorAction SilentlyContinue |
                Sort-Object Length -Descending |
                Select-Object -First 1
                if ($exe) { return $exe.FullName }
            }
        }
    }

    $shortcutRoots = @(
        "$env:APPDATA\Microsoft\Windows\Start Menu\Programs",
        "$env:ProgramData\Microsoft\Windows\Start Menu\Programs"
    ) | Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Container -ErrorAction SilentlyContinue) }

    foreach ($root in $shortcutRoots) {
        $shortcuts = Get-ChildItem -LiteralPath $root -Filter '*.lnk' -Recurse -ErrorAction SilentlyContinue
        foreach ($lnk in $shortcuts) {
            $stem = [System.IO.Path]::GetFileNameWithoutExtension($lnk.Name)
            $matched = $false
            foreach ($needle in $needleParts) {
                if ($stem -and $stem.IndexOf($needle, [StringComparison]::OrdinalIgnoreCase) -ge 0) { $matched = $true; break }
                if ($Name -and $stem -and $Name.IndexOf($stem, [StringComparison]::OrdinalIgnoreCase) -ge 0) { $matched = $true; break }
            }
            if ($matched) {
                $target = Get-ShortcutTargetPath $lnk.FullName
                if ($target) { return $target }
            }
        }
    }

    # Last-resort heuristic: guess EXE path from the app name / ID tail under
    # common install roots. Catches apps like TeamViewer that have a registry
    # entry with no DisplayIcon and no InstallLocation but install to a
    # predictable folder under %ProgramFiles%.
    $guessNames = @($Name, ($Id.Split('.')[-1])) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } | Select-Object -Unique
    $guessRoots = @(
        $env:ProgramFiles,
        ${env:ProgramFiles(x86)},
        $env:LOCALAPPDATA,
        (Join-Path $env:LOCALAPPDATA 'Programs')
    ) | Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Container -ErrorAction SilentlyContinue) }
    foreach ($gName in $guessNames) {
        foreach ($gRoot in $guessRoots) {
            $candidate = Join-Path $gRoot "$gName\$gName.exe"
            if (Test-Path -LiteralPath $candidate -PathType Leaf -ErrorAction SilentlyContinue) { return $candidate }
            # Also try with spaces removed (e.g. "TeamViewer" folder but "TeamViewer.exe")
            $folderMatch = Get-ChildItem -LiteralPath $gRoot -Directory -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -like "*$gName*" } |
            Select-Object -First 1
            if ($folderMatch) {
                $exe = Get-ChildItem -LiteralPath $folderMatch.FullName -Filter '*.exe' -File -ErrorAction SilentlyContinue |
                Where-Object { $_.Name -notmatch 'uninstall|setup|crash|helper|update' } |
                Sort-Object Length -Descending |
                Select-Object -First 1
                if ($exe) { return $exe.FullName }
            }
        }
    }

    return $null
}

function Get-AppIconData {
    param([string]$Id, [string]$Name)
    try {
        $cacheDir = Join-Path (Get-WinCommanderUserDataDir) "icon-cache"
        New-Item -ItemType Directory -Force -Path $cacheDir -ErrorAction SilentlyContinue | Out-Null
        $cachePath = Join-Path $cacheDir ((ConvertTo-IconCacheKey -Id $Id -Name $Name) + ".png")
        if (Test-Path -LiteralPath $cachePath -PathType Leaf -ErrorAction SilentlyContinue) {
            $bytes = [System.IO.File]::ReadAllBytes($cachePath)
            return "data:image/png;base64,$([Convert]::ToBase64String($bytes))"
        }

        $sourcePath = Resolve-AppIconSourcePath -Id $Id -Name $Name
        if (-not $sourcePath) { return $null }

        Add-Type -AssemblyName System.Drawing -ErrorAction SilentlyContinue
        $icon = [System.Drawing.Icon]::ExtractAssociatedIcon($sourcePath)
        if ($null -eq $icon) { return $null }
        $bitmap = $icon.ToBitmap()
        $bitmap.Save($cachePath, [System.Drawing.Imaging.ImageFormat]::Png)
        $ms = New-Object System.IO.MemoryStream
        $bitmap.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
        $encoded = [Convert]::ToBase64String($ms.ToArray())
        $ms.Dispose()
        $bitmap.Dispose()
        $icon.Dispose()
        return "data:image/png;base64,$encoded"
    }
    catch {
        return $null
    }
}

# Reset winget sources - fixes exit code -1978335138 (source unavailable) on new devices
function Invoke-WingetSourceReset {
    param([string]$WingetCmd)
    & $WingetCmd source reset --force 2>$null
    & $WingetCmd source update 2>$null
}

# Lightweight, proactive source sync. Fresh Windows installs ship winget with
# stale/empty source metadata, so the FIRST install or upgrade fails with
# -1978335138 (source data not found) before our reactive reset ever runs.
# `source update` (unlike `source reset --force`) only refreshes the index and
# preserves any custom sources, so it is safe to run before a batch operation.
function Invoke-WingetSourceUpdate {
    param([string]$WingetCmd)
    & $WingetCmd source update 2>$null
}

# Install selected apps via Winget (one at a time, winget source only)
# Install selected apps via Winget (Simplified / Linear)
function Install-WingetApps {
    param([string[]]$AppIds)

    Assert-IsAdmin

    # Deduce list from input (handle string vs array, and splitting combined strings)
    $list = $AppIds | ForEach-Object { $_.ToString().Split(',') } | ForEach-Object { $_.Trim() } | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }

    $installEverythingCli = $list -contains 'Voidtools.Everything'

    $needsWinget = $list | Where-Object { $_ -ne 'ImDisk.Toolkit' }
    $wingetCmd = $null
    if ($needsWinget) {
        $wingetCmd = Resolve-WingetPath
        if (-not $wingetCmd) {
            throw "Winget is not installed."
        }
        # Proactively sync sources so the first install on a fresh device does
        # not fail with -1978335138 before any package is even attempted.
        Invoke-WingetSourceUpdate -WingetCmd $wingetCmd
    }

    $results = @()
    foreach ($appId in $list) {
        if ($null -eq $appId) { continue }
        $appId = $appId.ToString().Trim()
        if ([string]::IsNullOrWhiteSpace($appId)) { continue }

        if ($appId -eq 'ImDisk.Toolkit') {
            $depResult = Install-RamDiskEngineDep
            $results += @{ id = $appId; status = if ($depResult.success) { 'installed' } else { 'failed' }; message = $depResult.message }
            continue
        }
        
        # Linear, one-at-time install. No background jobs, no fancy retries.
        # Added --force to ensure it tries even if it thinks it's there but broken.
        # Added --disable-interactivity to prevent hanging/blocking.
        # Use the call operator (&) instead of Start-Process: App Execution Alias
        # stubs (Microsoft\WindowsApps\winget.exe) cannot be launched via
        # Start-Process -FilePath and throw "The file cannot be accessed by the
        # system" (InvalidOperationException). The & operator resolves the alias.
        $cmdArgs = @("install", "--id", "$appId", "--exact", "--silent", "--source", "winget", "--accept-source-agreements", "--accept-package-agreements", "--force", "--disable-interactivity")

        & $wingetCmd @cmdArgs *>&1 | Out-Null
        $code = $LASTEXITCODE
        
        if ($code -eq 0) {
            $results += @{ id = $appId; status = 'installed' }
        }
        elseif ($code -eq -1978335212) {
            # 0x8A150014: "Already installed"
            $results += @{ id = $appId; status = 'already-installed' }
        }
        elseif ($code -eq -1978335231) {
            # 0x8A150041: installer hash mismatch — winget's community manifest
            # hasn't caught up with a newer installer binary. Retry once with
            # --ignore-security-hash before surfacing the error; this is safe
            # because we already ran a source update above.
            $retryArgs = @("install", "--id", "$appId", "--exact", "--silent", "--source", "winget", "--accept-source-agreements", "--accept-package-agreements", "--force", "--disable-interactivity", "--ignore-security-hash")
            & $wingetCmd @retryArgs *>&1 | Out-Null
            $retryCode = $LASTEXITCODE
            if ($retryCode -eq 0) {
                $results += @{ id = $appId; status = 'installed' }
            } elseif ($retryCode -eq -1978335212) {
                $results += @{ id = $appId; status = 'already-installed' }
            } else {
                # KT: return a clean, structured failure instead of throwing —
                # a thrown exception is caught by the generic command router
                # (core/router.ps1), which glues the full raw PowerShell stack
                # trace onto this message for every error in the app. This
                # failure is already fully diagnosed; the user doesn't need
                # "At line:69 char:17 ..." appended to a message that already
                # tells them exactly what happened and what to do about it.
                return @{
                    error   = $true
                    message = "Installer hash mismatch for $appId - winget's manifest is out of date. Try again after running 'winget source update', or install directly from the publisher."
                    id      = $appId
                    status  = 'failed'
                }
            }
        }
        else {
            # Same reasoning as the hash-mismatch case above: a clean, already-
            # diagnosed failure, not an unexpected script error — no stack trace.
            return @{
                error   = $true
                message = "Installation failed for $appId with exit code $code"
                id      = $appId
                status  = 'failed'
            }
        }
    }

    if ($installEverythingCli) {
        try {
            $cliPath = Install-EverythingSearchCli
            $results += @{ id = 'WinCommander.EverythingCli'; status = 'installed'; path = $cliPath }
        }
        catch {
            return @{
                error   = $true
                message = "Everything was installed, but its required search CLI could not be installed: $($_.Exception.Message)"
                id      = 'Voidtools.Everything'
                status  = 'failed'
            }
        }
    }
    
    return @{ status = 'success'; apps = $list; results = $results }
}

# Upgrade all apps via Winget
function Upgrade-AllApps {
    $wingetCmd = Resolve-WingetPath
    if (-not $wingetCmd) {
        throw "Winget is not installed."
    }

    # Proactively sync sources first so a fresh device's stale index does not
    # fail the whole batch with -1978335138.
    Invoke-WingetSourceUpdate -WingetCmd $wingetCmd

    # `--include-unknown` covers packages whose installed version winget can't
    # parse (common for direct-.exe installs like Claude). Dropping `--source winget`
    # lets msstore / msi-tracked entries upgrade through the same path.
    & $wingetCmd upgrade --all --silent --include-unknown --accept-source-agreements --accept-package-agreements
    if ($LASTEXITCODE -ne 0) {
        if ($LASTEXITCODE -eq -1978335138) {
            Invoke-WingetSourceReset -WingetCmd $wingetCmd
            & $wingetCmd upgrade --all --silent --include-unknown --accept-source-agreements --accept-package-agreements
        }
        # -1978335138 (source data not found / no applicable upgrade for the set)
        # and -1978335189 (no applicable update) are not user-action failures: in a
        # bulk `upgrade --all` some packages legitimately have no winget-applicable
        # update (self-updating apps like Brave / EdgeWebView2Runtime). Do not abort
        # the whole batch — the upgradable packages were already processed above.
        if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne -1978335138 -and $LASTEXITCODE -ne -1978335189) {
            throw "Winget upgrade failed (exit code $LASTEXITCODE)"
        }
    }
    return @{ status = 'success' }
}

# Upgrade a single app via Winget.
#
# Robustness: apps installed outside winget (e.g. Anthropic.Claude installed
# via the direct .exe download from anthropic.com) are listed by winget but
# match no winget-source candidate when `--source winget` is enforced - winget
# returns -1978335212 ("no packages found matching") and the upgrade silently
# becomes a no-op. We try a strict pass first, then progressively relax
# constraints (drop --source, drop --exact, add --include-unknown) so winget
# can match the package against any source it knows about.
function Upgrade-App {
    param([string]$AppId)

    $wingetCmd = Resolve-WingetPath
    if (-not $wingetCmd) {
        throw "Winget is not installed."
    }

    if ([string]::IsNullOrWhiteSpace($AppId)) {
        throw "App ID is required."
    }

    $candidateIds = @($AppId)
    if ($AppId -eq 'OpenJS.NodeJS') {
        $candidateIds += 'OpenJS.NodeJS.LTS'
    }

    $commonFlags = @('--silent', '--include-unknown',
        '--accept-source-agreements', '--accept-package-agreements')

    # Each tier broadens the match; we stop at the first non-"no-package-found"
    # exit code. winget writes progress to stdout - discard it so it doesn't
    # pollute the function's return value (the router serializes whatever this
    # function emits).
    $tryTiered = {
        param([string]$id)

        # Tier 1: strict - exact ID, default source resolution
        & $wingetCmd upgrade --id $id --exact @commonFlags *>$null
        $code = $LASTEXITCODE
        if ($code -ne -1978335212) { return $code }

        # Tier 2: substring match - handles ARP / sibling IDs
        & $wingetCmd upgrade --id $id @commonFlags *>$null
        $code = $LASTEXITCODE
        if ($code -ne -1978335212) { return $code }

        # Tier 3: name-based - last-resort match if ID never resolves
        & $wingetCmd upgrade $id @commonFlags *>$null
        return $LASTEXITCODE
    }.GetNewClosure()

    $lastCode = 0
    foreach ($candidateId in ($candidateIds | Select-Object -Unique)) {
        $lastCode = & $tryTiered $candidateId

        if ($lastCode -eq -1978335138) {
            Invoke-WingetSourceReset -WingetCmd $wingetCmd
            $lastCode = & $tryTiered $candidateId
        }

        if ($lastCode -eq 0) {
            return @{ status = 'success'; id = $candidateId }
        }

        # No matching package across any tier - try the next alias.
        if ($lastCode -eq -1978335212) {
            continue
        }

        # No applicable upgrade is not a failed user action. -1978335189 = "no
        # applicable update". -1978335138 = "source data not found" which, AFTER
        # the source reset+retry above has already failed, means winget simply
        # cannot produce an applicable upgrade for this package (typically a
        # self-updating / externally-installed app like Brave or
        # EdgeWebView2Runtime). Treat both as a benign no-op, not a hard error.
        if ($lastCode -eq -1978335189 -or $lastCode -eq -1978335138) {
            return @{ status = 'no-upgrade'; id = $candidateId }
        }

        throw "Winget upgrade failed for $candidateId (exit code $lastCode)"
    }

    # Every alias returned "no packages found". Surface this as a real failure
    # so the UI tells the user something went wrong instead of silently
    # claiming the upgrade succeeded - otherwise the same app reappears in the
    # pending list on the next inventory refresh and the user clicks again
    # forever.
    throw "Winget could not match an installed package for $AppId (this usually means the app was installed outside winget - try updating it from inside the app, or reinstalling via winget)."
}

# Check if Winget is installed
function Test-WingetInstalled {
    $installed = $null -ne (Resolve-WingetPath)
    return @{ status = if ($installed) { 'installed' } else { 'not-installed' } }
}

# ============================================================================
# Get-AppInventory - UNIFIED INVENTORY SNAPSHOT
# ============================================================================
# PURPOSE: Produces a single point-in-time snapshot of ALL apps on this PC.
#          Result is persisted to settings.json -> current.apps.inventory
#          so heartbeat can send cached data without re-shelling to winget.
#
# RETURNS: Raw hashtable (NOT JSON - router handles serialization).
#
# LEARNING: This replaces the need to call Get-AppStatus + Get-UpgradeList +
#           Get-EssentialAppsStatus separately. One call, one snapshot, one persist.
#           The scan takes 5-10s because of winget list + winget upgrade.
#           Don't call this on every heartbeat - run on startup + 60-min interval
#           + after install/upgrade/uninstall actions.
#
# SCHEMA:
#   { lastScanAt, scanDurationMs,
#     manifestApps[], otherApps[], pendingUpdates[],
#     essentials: { meshVpn, productivityEngine, winget },
#     summary: { totalInstalled, manifestInstalled, manifestTotal,
#                manifestMissing, otherInstalled, updatesAvailable, essentialsOk } }
# ============================================================================
function Get-AppInventory {
    $startTime = Get-Date

    # -- 1. Get manifest (hardcoded catalog of ~60 apps) --
    $manifest = Read-AppsManifest
    $manifestIds = @{}
    foreach ($app in $manifest) { $manifestIds[$app.id] = $true }

    # -- 2. Get all installed apps via winget list --
    $wingetCmd = Resolve-WingetPath
    $allInstalled = @()
    $allInstalledMap = @{}  # id -> parsed item (name, version, etc.)
    if ($wingetCmd) {
        try {
            $raw = & $wingetCmd list --accept-source-agreements --disable-interactivity 2>$null | Out-String
            $items = Parse-WingetTable -Raw $raw
            foreach ($item in $items) {
                if ($item.Id) {
                    $allInstalled += $item.Id
                    $allInstalledMap[$item.Id] = $item
                }
            }
        }
        catch { }
    }

    # -- 3. Get all upgradeable apps via winget upgrade --
    $upgradeMap = @{}  # id -> { version, availableVersion }
    if ($wingetCmd) {
        try {
            $rawUp = & $wingetCmd list --upgrade-available --accept-source-agreements --disable-interactivity 2>$null | Out-String
            $upItems = Parse-WingetTable -Raw $rawUp
            foreach ($item in $upItems) {
                if ($item.Id) {
                    $upgradeMap[$item.Id] = @{
                        version          = $item.Version
                        availableVersion = $item.Available
                    }
                }
            }
        }
        catch { }
    }

    # -- 3b. Filesystem fallback detection for apps that hide from winget --
    # LEARNING: Some apps (e.g. VeraCrypt) are intentionally installed outside winget's
    # view (portable, stealth, or manual installs). We detect them via filesystem/registry
    # so step 4 can mark them installed even if winget doesn't list them.
    # Maps manifest ID -> { installed: bool, version: string|null }
    $filesystemOverrides = @{}

    # VeraCrypt - often hidden by design
    $veraPath = "$env:ProgramFiles\VeraCrypt\VeraCrypt.exe"
    if (Test-Path $veraPath) {
        $veraVer = try { (Get-Item $veraPath).VersionInfo.ProductVersion } catch { $null }
        $filesystemOverrides['IDRIX.VeraCrypt'] = @{ installed = $true; version = $veraVer }
    }

    # Tailscale (Mesh) - check exe and PATH
    $meshInstalled = $false
    $meshVersion = $null
    if ((Get-Command tailscale.exe -ErrorAction SilentlyContinue) -or
        (Test-Path "$env:ProgramFiles\Tailscale\tailscale.exe") -or
        (Test-Path "$env:ProgramFiles (x86)\Tailscale\tailscale.exe")) {
        $meshInstalled = $true
        $meshExePath = if (Get-Command tailscale.exe -EA SilentlyContinue) { (Get-Command tailscale.exe).Source } elseif (Test-Path "$env:ProgramFiles\Tailscale\tailscale.exe") { "$env:ProgramFiles\Tailscale\tailscale.exe" } else { "$env:ProgramFiles (x86)\Tailscale\tailscale.exe" }
        $meshVersion = try { (Get-Item $meshExePath).VersionInfo.ProductVersion } catch { $null }
        $filesystemOverrides['Tailscale.Tailscale'] = @{ installed = $true; version = $meshVersion }
    }

    # ActivityWatch - check multiple paths
    $awInstalled = $false
    $awExePath = $null
    $awPaths = @(
        "$env:LOCALAPPDATA\Programs\ActivityWatch\aw-server.exe",
        "$env:ProgramFiles\ActivityWatch\aw-server.exe",
        "${env:ProgramFiles(x86)}\ActivityWatch\aw-server.exe",
        "$env:LOCALAPPDATA\Programs\ActivityWatch\aw-server\aw-server.exe"
    )
    foreach ($p in $awPaths) { if (Test-Path $p) { $awInstalled = $true; $awExePath = $p; break } }
    if (-not $awInstalled -and (Get-Command "aw-server" -ErrorAction SilentlyContinue)) {
        $awInstalled = $true
        $awExePath = (Get-Command "aw-server").Source
    }
    if (-not $awInstalled -and (Test-Path "$env:LOCALAPPDATA\Microsoft\WinGet\Packages")) {
        $awDirs = Get-ChildItem -Path "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Directory -EA SilentlyContinue | Where-Object { $_.Name -like "ActivityWatch*" }
        foreach ($d in $awDirs) {
            $found = Get-ChildItem -Path $d.FullName -Filter "aw-server.exe" -Recurse -EA SilentlyContinue | Select-Object -First 1
            if ($found) { $awInstalled = $true; $awExePath = $found.FullName; break }
        }
    }
    if ($awInstalled) {
        $awVer = if ($awExePath) { try { (Get-Item $awExePath).VersionInfo.ProductVersion } catch { $null } } else { $null }
        $filesystemOverrides['ActivityWatch.ActivityWatch'] = @{ installed = $true; version = $awVer }
    }

    # RAM Disk Engine - detect the RAM-disk engine even though it installs outside winget.
    $imdiskExe = $null
    $imdiskCandidates = @(
        (Get-Command imdisk.exe -ErrorAction SilentlyContinue).Source,
        "$env:ProgramW6432\ImDisk\imdisk.exe",
        "$env:ProgramFiles\ImDisk\imdisk.exe",
        "${env:ProgramFiles(x86)}\ImDisk\imdisk.exe",
        "$env:SystemRoot\System32\imdisk.exe"
    ) | Where-Object { $_ -and $_.Trim() -ne '' }
    foreach ($candidate in $imdiskCandidates) {
        if (Test-Path $candidate) { $imdiskExe = $candidate; break }
    }
    if ($imdiskExe) {
        $imdiskVer = try { (Get-Item $imdiskExe).VersionInfo.ProductVersion } catch { $null }
        $filesystemOverrides['ImDisk.Toolkit'] = @{ installed = $true; version = $imdiskVer }
    }

    # -- 4. Classify manifest apps (installed/not, update available) --
    # LEARNING: Fuzzy matching detects apps registered under ARP (Add/Remove Programs)
    # IDs that differ from the winget manifest ID. When a fuzzy match hits, we must
    # capture the ACTUAL installed ID so we can look up version info from allInstalledMap.
    $manifestApps = @()
    $manifestInstalledCount = 0
    $fuzzyMatchedIds = @{}  # Track ARP IDs consumed by fuzzy matching (for step 5 dedup)
    foreach ($app in $manifest) {
        # Check if installed - exact match first
        $isInstalled = $allInstalled -contains $app.id
        $matchedInstId = if ($isInstalled) { $app.id } else { $null }

        # Fuzzy match for ARP-style IDs
        if (-not $isInstalled) {
            $idParts = $app.id -split '\.'
            $publisher = $idParts[0]
            $appName = $idParts[-1]
            foreach ($instId in $allInstalled) {
                if ($instId -match '^ARP\\') {
                    if ($instId -match [regex]::Escape($appName)) { $isInstalled = $true; $matchedInstId = $instId; break }
                    if ($instId -match [regex]::Escape($publisher)) { $isInstalled = $true; $matchedInstId = $instId; break }
                }
            }
        }

        # Special cases for common apps with non-standard ARP names
        if (-not $isInstalled) {
            $specialCases = @{
                'OpenJS.NodeJS'         = 'Node\.js|NodeJS'
                'Git.Git'               = 'Git_is1|\\Git\b'
                'Ablaze.Floorp'         = 'Floorp'
                'AutoHotkey.AutoHotkey' = 'AutoHotkey'
                'Balena.Etcher'         = 'Etcher|balenaEtcher'
            }
            if ($specialCases.ContainsKey($app.id)) {
                $pattern = $specialCases[$app.id]
                foreach ($instId in $allInstalled) {
                    if ($instId -match $pattern) { $isInstalled = $true; $matchedInstId = $instId; break }
                }
            }
        }

        # Filesystem fallback - catches apps hidden from winget (portable/stealth installs)
        if (-not $isInstalled -and $filesystemOverrides.ContainsKey($app.id)) {
            $isInstalled = $true
            $matchedInstId = '__filesystem__'
        }

        if ($isInstalled) {
            $manifestInstalledCount++
            # Track fuzzy-matched IDs so step 5 doesn't double-count them as "other apps"
            if ($matchedInstId -and $matchedInstId -ne $app.id) {
                $fuzzyMatchedIds[$matchedInstId] = $true
            }
        }

        # Get version info - check upgrade map by manifest ID first, then by matched ID,
        # then fall back to installed map by either ID
        $hasUpdate = $upgradeMap.ContainsKey($app.id)
        $installedVersion = $null
        $latestVersion = $null
        if ($hasUpdate) {
            $installedVersion = $upgradeMap[$app.id].version
            $latestVersion = $upgradeMap[$app.id].availableVersion
        }
        elseif ($matchedInstId -and $upgradeMap.ContainsKey($matchedInstId)) {
            $hasUpdate = $true
            $installedVersion = $upgradeMap[$matchedInstId].version
            $latestVersion = $upgradeMap[$matchedInstId].availableVersion
        }
        elseif ($isInstalled) {
            # No update - pull installed version from whichever ID matched
            if ($allInstalledMap.ContainsKey($app.id)) {
                $installedVersion = $allInstalledMap[$app.id].Version
            }
            elseif ($matchedInstId -and $matchedInstId -ne '__filesystem__' -and $allInstalledMap.ContainsKey($matchedInstId)) {
                $installedVersion = $allInstalledMap[$matchedInstId].Version
            }
            # Last resort: filesystem-detected version (for apps hidden from winget)
            if (-not $installedVersion -and $filesystemOverrides.ContainsKey($app.id)) {
                $installedVersion = $filesystemOverrides[$app.id].version
            }
        }

        $manifestApps += @{
            id               = $app.id
            name             = $app.name
            description      = $app.description
            category         = $app.category
            installed        = $isInstalled
            installedVersion = $installedVersion
            latestVersion    = $latestVersion
            updateAvailable  = $hasUpdate
            iconData         = if ($isInstalled) { Get-AppIconData -Id $app.id -Name $app.name } else { $null }
        }
    }

    # -- 5. Classify other apps (installed but NOT in manifest) --
    $otherApps = @()
    foreach ($instId in $allInstalled) {
        if (-not $manifestIds.ContainsKey($instId) -and -not $fuzzyMatchedIds.ContainsKey($instId)) {
            # Skip exact manifest IDs AND fuzzy-matched ARP IDs (already counted above)
            $item = $allInstalledMap[$instId]
            $hasUpdate = $upgradeMap.ContainsKey($instId)
            $latestVersion = $null
            $installedVersion = if ($item) { $item.Version } else { $null }
            if ($hasUpdate) {
                $installedVersion = $upgradeMap[$instId].version
                $latestVersion = $upgradeMap[$instId].availableVersion
            }

            $otherApps += @{
                id               = $instId
                name             = if ($item) { $item.Name } else { $null }
                installedVersion = $installedVersion
                latestVersion    = $latestVersion
                updateAvailable  = $hasUpdate
                iconData         = Get-AppIconData -Id $instId -Name $(if ($item) { $item.Name } else { $null })
            }
        }
    }

    # -- 6. Build pendingUpdates (flat list for quick admin view) --
    $pendingUpdates = @()
    foreach ($app in $manifestApps) {
        if ($app.updateAvailable) {
            $pendingUpdates += @{
                id               = $app.id
                name             = $app.name
                installedVersion = $app.installedVersion
                latestVersion    = $app.latestVersion
                source           = 'winget'
                inManifest       = $true
                iconData         = $app.iconData
            }
        }
    }
    foreach ($app in $otherApps) {
        if ($app.updateAvailable) {
            $pendingUpdates += @{
                id               = $app.id
                name             = $app.name
                installedVersion = $app.installedVersion
                latestVersion    = $app.latestVersion
                source           = 'winget'
                inManifest       = $false
                iconData         = $app.iconData
            }
        }
    }

    # -- 7. Check essentials (reuse step 3b filesystem detection) --
    # LEARNING: Step 3b already detected Tailscale and ActivityWatch via filesystem.
    # Reuse those results here to avoid duplicate I/O. Only connectivity/running checks are new.
    # Tailscale: reuse $meshInstalled/$meshVersion from step 3b, only add connected check
    $meshConnected = $null
    if ($meshInstalled) {
        try {
            $meshExe = if (Get-Command tailscale.exe -ErrorAction SilentlyContinue) { "tailscale" } else { "$env:ProgramFiles\Tailscale\tailscale.exe" }
            $statusJson = & $meshExe status --json 2>$null | Out-String | ConvertFrom-Json
            $meshConnected = $statusJson.BackendState -eq "Running"
        }
        catch {
            $meshConnected = $false
        }
    }

    # ActivityWatch: reuse $awInstalled from step 3b, only add running check
    $awRunning = $false
    if ($awInstalled) {
        $awRunning = $null -ne (Get-Process -Name "aw-server" -ErrorAction SilentlyContinue)
    }

    $wingetInstalled = $null -ne $wingetCmd
    $wingetVersion = $null
    if ($wingetInstalled) {
        try {
            $wv = & $wingetCmd --version 2>$null
            $wingetVersion = ($wv -replace '^v', '').Trim()
        }
        catch { }
    }

    $essentials = @{
        meshVpn            = @{ installed = $meshInstalled; version = $meshVersion; connected = $meshConnected }
        productivityEngine = @{ installed = $awInstalled; running = $awRunning }
        winget             = @{ installed = $wingetInstalled; version = $wingetVersion }
    }

    $essentialsOk = $meshInstalled -and $awInstalled -and $wingetInstalled

    # -- 8. Build summary --
    $otherInstalledCount = $otherApps.Count
    $totalInstalled = $manifestInstalledCount + $otherInstalledCount

    $summary = @{
        totalInstalled    = $totalInstalled
        manifestInstalled = $manifestInstalledCount
        manifestTotal     = $manifest.Count
        manifestMissing   = $manifest.Count - $manifestInstalledCount
        otherInstalled    = $otherInstalledCount
        updatesAvailable  = $pendingUpdates.Count
        essentialsOk      = $essentialsOk
    }

    $endTime = Get-Date
    $durationMs = [int](($endTime - $startTime).TotalMilliseconds)

    # Return raw hashtable - router handles JSON serialization
    return @{
        lastScanAt     = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
        scanDurationMs = $durationMs
        manifestApps   = $manifestApps
        otherApps      = $otherApps
        pendingUpdates = $pendingUpdates
        essentials     = $essentials
        summary        = $summary
    }
}

# Install Winget package manager
function Install-Winget {
    Assert-IsAdmin
    if ((Test-WingetInstalled).status -eq 'installed') {
        return @{ status = 'installed' }
    }

    Set-PSRepository -Name PSGallery -InstallationPolicy Trusted -ErrorAction SilentlyContinue
    Install-PackageProvider -Name NuGet -Force -ErrorAction SilentlyContinue
    Install-Module Microsoft.WinGet.Client -Force -ErrorAction SilentlyContinue
    Import-Module Microsoft.WinGet.Client -ErrorAction SilentlyContinue
    Repair-WinGetPackageManager -ErrorAction SilentlyContinue

    return @{ status = 'installed' }
}
