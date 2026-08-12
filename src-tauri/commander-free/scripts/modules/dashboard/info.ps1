# ============================================================================
# SYSTEM - INFO MODULE
# Retrieves Detailed context about the Windows operating system and hardware
# ============================================================================

# Get-BatteryHealth — exposes battery design/full-charge capacity and cycle
# count via `powercfg /batteryreport`. Returns:
#   { status: 'no-battery' }                  — desktop / detached battery
#   { status: 'unavailable', error: '...' }   — laptop but report failed
#   { status: 'ok', present: true, healthPct, designMwh, fullChargeMwh,
#     cycleCount, manufacturer, model, chemistry, reportedAt }
# Cached on the frontend; report generation takes ~1-3 s.
function Get-BatteryHealth {
    try {
        $batteries = Get-CimInstance -ClassName Win32_Battery -ErrorAction SilentlyContinue
        if (-not $batteries) {
            return @{ status = 'no-battery'; present = $false }
        }

        $tmpDir = $env:TEMP
        if (-not $tmpDir) { $tmpDir = [System.IO.Path]::GetTempPath() }
        $reportPath = Join-Path $tmpDir ("wc-battery-" + [guid]::NewGuid().ToString('N') + ".xml")

        try {
            # /xml emits a structured report we can parse without HTML scraping.
            # Out-Null suppresses the "Battery life report saved to ..." line.
            $null = & powercfg.exe /batteryreport /xml /output $reportPath 2>&1
            if (-not (Test-Path $reportPath)) {
                return @{ status = 'unavailable'; present = $true; error = 'powercfg did not produce a report' }
            }

            [xml]$doc = Get-Content -Path $reportPath -Raw -Encoding UTF8

            # The XML namespace varies by Windows build, so use local-name() XPath.
            $ns = New-Object System.Xml.XmlNamespaceManager($doc.NameTable)
            # Battery list — most laptops report a single battery, but multi-battery
            # devices (Surface Book, ThinkPads with secondary) report two. Sum them.
            $batteryNodes = $doc.SelectNodes("//*[local-name()='Battery']")
            if (-not $batteryNodes -or $batteryNodes.Count -eq 0) {
                return @{ status = 'unavailable'; present = $true; error = 'no battery node in report' }
            }

            $designSum = 0
            $fullSum = 0
            $cycleSum = 0
            $cycleSeen = $false
            $manufacturer = $null
            $model = $null
            $chemistry = $null

            foreach ($b in $batteryNodes) {
                $designStr = ($b.SelectSingleNode("*[local-name()='DesignCapacity']")).'#text'
                $fullStr   = ($b.SelectSingleNode("*[local-name()='FullChargeCapacity']")).'#text'
                $cycleNode = $b.SelectSingleNode("*[local-name()='CycleCount']")
                $manuNode  = $b.SelectSingleNode("*[local-name()='Manufacturer']")
                $modelNode = $b.SelectSingleNode("*[local-name()='Id']")
                $chemNode  = $b.SelectSingleNode("*[local-name()='Chemistry']")

                if ($designStr) { $designSum += [int]$designStr }
                if ($fullStr)   { $fullSum   += [int]$fullStr }
                if ($cycleNode -and $cycleNode.'#text') {
                    $cycleSum += [int]$cycleNode.'#text'
                    $cycleSeen = $true
                }
                if (-not $manufacturer -and $manuNode) { $manufacturer = $manuNode.'#text' }
                if (-not $model        -and $modelNode){ $model        = $modelNode.'#text' }
                if (-not $chemistry    -and $chemNode) { $chemistry    = $chemNode.'#text' }
            }

            $healthPct = if ($designSum -gt 0) {
                [Math]::Round(($fullSum / $designSum) * 100, 1)
            } else { $null }

            return @{
                status        = 'ok'
                present       = $true
                healthPct     = $healthPct
                designMwh     = $designSum
                fullChargeMwh = $fullSum
                cycleCount    = if ($cycleSeen) { $cycleSum } else { $null }
                manufacturer  = $manufacturer
                model         = $model
                chemistry     = $chemistry
                reportedAt    = (Get-Date).ToUniversalTime().ToString("o")
            }
        } finally {
            Remove-Item -Path $reportPath -Force -ErrorAction SilentlyContinue
        }
    }
    catch {
        return @{ status = 'unavailable'; present = $true; error = $_.Exception.Message }
    }
}


# Get comprehensive OS information and system uptime
function Get-SystemInfo {
    try {
        # Use TickCount64 for accurate uptime (handles Fast Startup) - 0ms cost
        $uptimeMilliseconds = [Environment]::TickCount64
        $uptimeSpan = [TimeSpan]::FromMilliseconds($uptimeMilliseconds)

        $osReg = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -ErrorAction SilentlyContinue
        
        # Get build number
        $buildNumber = if ($osReg.CurrentBuildNumber) { [int]$osReg.CurrentBuildNumber } else { 0 }
        $ubr = $osReg.UBR
        $buildFull = if ($null -ne $ubr) { "$buildNumber.$ubr" } else { "$buildNumber" }
        
        # Detect Windows 11 vs 10 (22000+ is Windows 11)
        $isWin11 = $buildNumber -ge 22000
        $productName = if ($osReg.ProductName) { $osReg.ProductName } else { "Windows" }
        
        # Display version (24H2, 23H2, etc.)
        $displayVersion = if ($osReg.DisplayVersion) { $osReg.DisplayVersion } elseif ($osReg.ReleaseId) { $osReg.ReleaseId } else { "" }
        
        # Build proper OS name
        # Registry 'ProductName' often says 'Windows 10 Pro' even on Win11, so we verify manually
        if ($isWin11 -and $productName -match "Windows 10") {
            $productName = $productName -replace "Windows 10", "Windows 11"
        }
        $osName = "$productName $displayVersion".Trim()

        $computerName = [System.Net.Dns]::GetHostName()
        $deviceType = "Unknown"

        # Detect laptop/desktop using enclosure chassis first, with OS battery fallback.
        $enclosure = Get-CimInstance -ClassName Win32_SystemEnclosure -ErrorAction SilentlyContinue
        if ($enclosure -and $enclosure.ChassisTypes) {
            $portableChassisTypes = @(8, 9, 10, 11, 12, 14, 18, 21, 30, 31, 32)
            $isPortableChassis = @($enclosure.ChassisTypes) | Where-Object { $portableChassisTypes -contains [int]$_ } | Select-Object -First 1
            if ($isPortableChassis) {
                $deviceType = "Laptop"
            }
            else {
                $deviceType = "Desktop"
            }
        }
        else {
            # Fallback: PortableOperatingSystem is usually true on battery-capable systems.
            $computerSystem = Get-CimInstance -ClassName Win32_ComputerSystem -ErrorAction SilentlyContinue
            if ($computerSystem -and $null -ne $computerSystem.PCSystemType) {
                switch ([int]$computerSystem.PCSystemType) {
                    2 { $deviceType = "Laptop" }
                    1 { $deviceType = "Desktop" }
                    default { $deviceType = "Unknown" }
                }
            }
        }
        
        # Hardware Info Probes.
        # NOTE: cpuUsage, cpuTemp and ramUsage are intentionally NOT computed here.
        # The Rust sysinfo poll (get_live_metrics, every 2s) is the source of truth
        # for those volatile values and overwrites them in the UI immediately. We
        # used to query Win32_PerfFormattedData_Counters_ThermalZoneInformation and
        # Win32_Processor.LoadPercentage here — each costs ~2.5s on servers/VMs and
        # added nothing the Rust poll didn't already provide. Returning 0 as a
        # placeholder keeps the shape intact without the cost.
        $cpuRaw = try { (Get-CimInstance -ClassName Win32_Processor -EA SilentlyContinue | Select-Object -First 1) } catch { $null }
        $cpu = if ($cpuRaw) { $cpuRaw.Name -replace '\s+with Radeon.*Graphics', '' } else { "Unknown" }
        $cpuUsage = 0
        $cpuTemp = 0

        $totalRam = try { (Get-CimInstance -ClassName Win32_ComputerSystem -EA SilentlyContinue).TotalPhysicalMemory } catch { 0 }
        $totalRamGb = $totalRam / 1GB
        $ramUsage = 0
        $ramText = "$([Math]::Round($totalRamGb, 0)) GB"
        
        # Refined GPU detection: prioritize discrete GPUs, exclude virtual
        $gpus = try { 
            Get-CimInstance -ClassName Win32_VideoController -EA SilentlyContinue | Where-Object { 
                ($_.AdapterCompatibility -match "NVIDIA" -or $_.AdapterCompatibility -match "AMD" -or $_.AdapterCompatibility -match "Intel" -or $_.PNPDeviceID -match "PCI") -and 
                $_.Name -notmatch "Parsec" -and 
                $_.Name -notmatch "Basic Render" -and
                $_.Name -notmatch "RDP" -and
                $_.Name -notmatch "Indirect"
            }
        }
        catch { $null }
        
        if (-not $gpus) {
            $gpus = try { Get-CimInstance -ClassName Win32_VideoController -EA SilentlyContinue | Where-Object { $_.Name -notmatch "Parsec" } | Select-Object -First 1 } catch { $null }
        }
        $gpuNames = if ($gpus) { $gpus.Name | Select-Object -Unique } else { @("Unknown") }
        $gpu = $gpuNames -join " / "
        
        # Structured disk info for frontend progress bars
        $disks = try {
            $drives = Get-CimInstance -ClassName Win32_LogicalDisk -Filter "DriveType=3" -EA SilentlyContinue
            $diskList = New-Object System.Collections.Generic.List[PSObject]
            if ($drives) {
                foreach ($d in $drives) {
                    $total = [Math]::Round($d.Size / 1GB, 1)
                    $free = [Math]::Round($d.FreeSpace / 1GB, 1)
                    $used = [Math]::Round(($d.Size - $d.FreeSpace) / 1GB, 1)
                    $percent = if ($d.Size -gt 0) { [Math]::Round(($used / $total) * 100, 0) } else { 0 }
                    $diskList.Add(@{
                            id      = $d.DeviceID
                            label   = "$($d.DeviceID) Drive"
                            totalGb = $total
                            usedGb  = $used
                            freeGb  = $free
                            percent = $percent
                        })
                }
            }
            $diskList.ToArray()
        }
        catch { @() }

        
        return @{
            osName      = $osName
            osVersion   = $displayVersion
            buildNumber = $buildFull
            hostname    = $computerName
            deviceType  = $deviceType
            isAdmin     = Test-IsAdmin
            cpu         = $cpu
            cpuUsage    = [int]$cpuUsage
            cpuTemp     = [int]$cpuTemp
            ram         = $ramText
            ramUsage    = [int]$ramUsage
            gpu         = $gpu
            disks       = $disks
            uptime      = @{
                days    = $uptimeSpan.Days
                hours   = $uptimeSpan.Hours
                minutes = $uptimeSpan.Minutes
            }
        }
    }
    catch {
        return @{
            osName      = "Unknown"
            osVersion   = "Unknown"
            buildNumber = "Unknown"
            hostname    = $env:COMPUTERNAME
            deviceType  = "Unknown"
            isAdmin     = $false
            uptime      = @{ days = 0; hours = 0; minutes = 0 }
        }
    }
}

function Get-VersionUpdate {
    try {
        # Strict versioning from package.json
        $currentVersion = (Get-Content (Resolve-DataPath "package.json") | ConvertFrom-Json).version
        # Production Update Endpoint
        $apiUrl = "https://winupdates.servalabs.com/latest.json"
        
        $headers = @{ 'User-Agent' = 'WinCommander-Check'; 'Accept' = 'application/vnd.github.v3+json' }
        $response = Invoke-RestMethod -Uri $apiUrl -Headers $headers -ErrorAction Stop
        
        # Determine version from R2 (latest.json) or GitHub response
        $latestVersion = if ($response.version) { $response.version } else { $response.tag_name -replace '^v', '' }
        $downloadUrl = if ($response.html_url) { $response.html_url } else { $response.platforms.'windows-x86_64'.url }
        
        @{
            currentVersion  = $currentVersion
            latestVersion   = $latestVersion
            updateAvailable = ([version]$latestVersion -gt [version]$currentVersion)
            downloadUrl     = $downloadUrl
            releaseName     = if ($response.name) { $response.name } else { "Version $latestVersion" }
            releaseNotes    = if ($response.body) { $response.body } else { $response.notes }
        }
    }
    catch {
        $currentVersion = (Get-Content (Resolve-DataPath "package.json") | ConvertFrom-Json).version
        @{ error = $true; message = $_.Exception.Message; currentVersion = $currentVersion; updateAvailable = $false }
    }
}

function Open-UpdatePage {
    param([string]$Url)

    if ([string]::IsNullOrWhiteSpace($Url)) {
        $Url = "https://winupdates.servalabs.com/latest.json"
    }
    if ($Url -notmatch '^https?://') {
        return @{ error = $true; message = "Only http/https update URLs can be opened." }
    }

    try {
        Start-Process $Url
        @{ status = "opened"; url = $Url }
    }
    catch {
        @{ error = $true; message = "Failed to open update page: $($_.Exception.Message)"; url = $Url }
    }
}

function Set-OEMInformation {
    param($Model = "PvtWinOS", $Manufacturer = "ServaLabs", $SupportURL = "https://servalabs.com", $SupportProvider = "ServaLabs Support")
    Assert-IsAdmin
    try {
        $path = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\OEMInformation"
        if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
        Set-ItemProperty -Path $path -Name "Model" -Value $Model -Force
        Set-ItemProperty -Path $path -Name "Manufacturer" -Value $Manufacturer -Force
        Set-ItemProperty -Path $path -Name "SupportURL" -Value $SupportURL -Force
        Set-ItemProperty -Path $path -Name "SupportProvider" -Value $SupportProvider -Force
        return @{ success = $true }
    }
    catch {
        return @{ error = $true; message = $_.Exception.Message }
    }
}

function Get-StorageStats {
    # Scans ALL fixed local drives for common document type groups.
    # Uses Everything CLI (es.exe) when available for near-instant results.
    # Falls back to .NET EnumerateFiles in parallel jobs (one per top-level dir).
    # Excludes: Windows, Program Files, ProgramData, AppData, junctions/symlinks,
    #           System Volume Information, Recovery, Recycle Bin.
    try {
        # ── System paths to always exclude ────────────────────────────────────
        $rawExcludes = @(
            $env:SystemRoot,
            $env:ProgramFiles,
            ${env:ProgramFiles(x86)},
            $env:ProgramData,
            $env:LOCALAPPDATA,
            $env:APPDATA,
            (Join-Path $env:SystemDrive 'System Volume Information'),
            (Join-Path $env:SystemDrive 'Recovery'),
            (Join-Path $env:SystemDrive '$Recycle.Bin'),
            (Join-Path $env:SystemDrive 'Windows.old')
        )
        $excludePaths = $rawExcludes | Where-Object { $_ } | ForEach-Object { $_.TrimEnd('\').ToLower() }

        # ── File type groups ───────────────────────────────────────────────────
        $typeDefs = [ordered]@{
            documents     = @('.doc', '.docx', '.odt', '.rtf', '.txt', '.pages')
            pdfs          = @('.pdf')
            spreadsheets  = @('.xls', '.xlsx', '.csv', '.ods', '.numbers')
            presentations = @('.ppt', '.pptx', '.odp', '.key')
            images        = @('.jpg', '.jpeg', '.png', '.gif', '.bmp', '.webp', '.heic', '.raw', '.tiff', '.svg')
            videos        = @('.mp4', '.mkv', '.avi', '.mov', '.wmv', '.flv', '.webm', '.m4v', '.ts')
            audio         = @('.mp3', '.wav', '.flac', '.aac', '.ogg', '.m4a', '.wma', '.opus')
            archives      = @('.zip', '.rar', '.7z', '.tar', '.gz', '.bz2', '.xz')
            code          = @('.py', '.js', '.ts', '.cs', '.java', '.cpp', '.c', '.go', '.rs', '.php', '.html', '.css', '.sh', '.ps1')
        }

        $result = [ordered]@{}

        # ── Try Everything CLI (Voidtools es.exe) for instant results ─────────
        $ev = [System.Environment]::GetEnvironmentVariable('EVERYTHING_CLI', 'Machine')
        $esCli = @(
            (Get-Command es.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -ErrorAction SilentlyContinue),
            "$env:ProgramFiles\Everything\es.exe",
            "$env:ProgramFiles\Everything 1.5a\es.exe",
            "${env:ProgramFiles(x86)}\Everything\es.exe",
            "$env:LOCALAPPDATA\Programs\Everything\es.exe",
            "$env:LOCALAPPDATA\Everything\es.exe",
            "$env:LOCALAPPDATA\Microsoft\WinGet\Links\es.exe",
            "$env:APPDATA\Everything\es.exe",
            "C:\Tools\es.exe",
            "C:\ProgramData\chocolatey\bin\es.exe",
            $(if ($ev) { "$ev\es.exe" } else { $null })
        ) | Where-Object { $_ -and (Test-Path $_ -ErrorAction SilentlyContinue) } | Select-Object -First 1

        # Only use Everything CLI if the Everything process is actually running.
        # es.exe exits silently (error to stderr) when Everything isn't running,
        # which makes every query return 0 — a false "no files found" result.
        $everythingRunning = $esCli -and (Get-Process -Name "Everything" -ErrorAction SilentlyContinue | Select-Object -First 1)

        $useNativeScan = -not $everythingRunning

        if ($everythingRunning) {
            foreach ($entry in $typeDefs.GetEnumerator()) {
                $extJoined = ($entry.Value | ForEach-Object { $_ -replace '^\.' }) -join ';'
                $query = "ext:$extJoined"

                # Count: pipe results and count lines
                $lines = & $esCli $query 2>$null
                $count = if ($lines) { @($lines).Count } else { 0 }

                # Size: use -size flag which prefixes each line with "  size  path"
                $sizeBytes = [long]0
                try {
                    $sizeLines = & $esCli -size $query 2>$null
                    foreach ($ln in $sizeLines) {
                        # Format: "      1,234,567 C:\path\to\file.mp4"
                        if ($ln -match '^\s*([\d,]+)\s+') {
                            $sizeBytes += [long]($Matches[1] -replace ',', '')
                        }
                    }
                }
                catch {}

                $result[$entry.Key] = @{
                    count  = $count
                    sizeGb = [Math]::Round($sizeBytes / 1GB, 2)
                    source = 'everything'
                }
            }

            # If Everything returned 0 for every category it is likely not yet
            # indexed (or using an older index that missed drives). Fall through
            # to the native .NET scanner so we never show a false all-zeros result.
            $totalEverythingCount = 0
            foreach ($v in $result.Values) { $totalEverythingCount += [int]$v.count }
            if ($totalEverythingCount -eq 0) {
                $result = [ordered]@{}
                $useNativeScan = $true
            }
        }

        if ($useNativeScan) {
            # ── Native scan: targets user-profile dirs + non-system drive roots ──
            # The old approach (BFS over ALL top-level dirs via Start-Job) times out
            # on large drives (6M+ files in C:\Users exceeds 90s). Instead we scan:
            #   1. Known user-profile folders (fast, covers 95% of user media)
            #   2. Top-level dirs on non-system drives (D:, E:, etc.)
            # AppData is still excluded to avoid log/cache noise.

            $typeKeys = @($typeDefs.Keys)
            $extMap = @{}
            foreach ($entry in $typeDefs.GetEnumerator()) {
                foreach ($ext in $entry.Value) { $extMap[$ext] = $entry.Key }
            }

            $totCounts = @{}; $totSizes = @{}
            foreach ($k in $typeKeys) { $totCounts[$k] = 0; $totSizes[$k] = [long]0 }

            # ── Build scan roots ───────────────────────────────────────────────
            $scanRoots = [System.Collections.Generic.List[string]]::new()

            # 1. Well-known user shell folders for current user
            $shellFolders = @(
                [Environment]::GetFolderPath('MyDocuments'),
                [Environment]::GetFolderPath('MyPictures'),
                [Environment]::GetFolderPath('MyVideos'),
                [Environment]::GetFolderPath('MyMusic'),
                [Environment]::GetFolderPath('Desktop'),
                (Join-Path ([Environment]::GetFolderPath('UserProfile')) 'Downloads'),
                (Join-Path ([Environment]::GetFolderPath('UserProfile')) 'OneDrive'),
                (Join-Path ([Environment]::GetFolderPath('UserProfile')) 'Google Drive'),
                (Join-Path ([Environment]::GetFolderPath('UserProfile')) 'iCloudDrive')
            )
            foreach ($f in $shellFolders) {
                if ($f -and (Test-Path -LiteralPath $f -PathType Container)) {
                    $scanRoots.Add($f)
                }
            }

            # 2. Top-level dirs on non-system fixed drives (D:, E:, etc.)
            $systemRoot = [System.IO.Path]::GetPathRoot([Environment]::GetFolderPath('Windows')).TrimEnd('\').ToLower()
            $fixedDrives = [System.IO.DriveInfo]::GetDrives() |
                Where-Object { $_.DriveType -eq 'Fixed' -and $_.IsReady }
            foreach ($drive in $fixedDrives) {
                $driveRoot = $drive.RootDirectory.FullName.TrimEnd('\').ToLower()
                if ($driveRoot -eq $systemRoot) { continue }   # skip C:\ — covered by shell folders above
                try {
                    foreach ($dir in [System.IO.Directory]::GetDirectories($drive.RootDirectory.FullName)) {
                        try {
                            $a = [System.IO.File]::GetAttributes($dir)
                            if (-not ($a -band [System.IO.FileAttributes]::ReparsePoint)) {
                                $scanRoots.Add($dir)
                            }
                        }
                        catch {}
                    }
                }
                catch {}
            }

            # ── Synchronous BFS over each root (no Start-Job) ─────────────────
            foreach ($scanRoot in $scanRoots) {
                $queue = [System.Collections.Generic.Queue[string]]::new()
                $queue.Enqueue($scanRoot)

                while ($queue.Count -gt 0) {
                    $dir = $queue.Dequeue()
                    try {
                        foreach ($f in [System.IO.Directory]::EnumerateFiles($dir)) {
                            $ext = [System.IO.Path]::GetExtension($f).ToLower()
                            if ($extMap.ContainsKey($ext)) {
                                $grp = $extMap[$ext]
                                $totCounts[$grp]++
                                try { $totSizes[$grp] += [System.IO.FileInfo]::new($f).Length } catch {}
                            }
                        }
                    }
                    catch {}
                    try {
                        foreach ($sub in [System.IO.Directory]::EnumerateDirectories($dir)) {
                            $subLower = $sub.ToLower()
                            if ($subLower -match '\\appdata\\' -or $subLower -match '\\appdata$') { continue }
                            try {
                                $a = [System.IO.File]::GetAttributes($sub)
                                if (-not ($a -band [System.IO.FileAttributes]::ReparsePoint)) {
                                    $queue.Enqueue($sub)
                                }
                            }
                            catch {}
                        }
                    }
                    catch {}
                }
            }

            foreach ($k in $typeKeys) {
                $result[$k] = @{
                    count  = $totCounts[$k]
                    sizeGb = [Math]::Round($totSizes[$k] / 1GB, 2)
                    source = 'scan'
                }
            }
        }

        return $result
    }
    catch {
        return @{ error = $true; message = $_.Exception.Message }
    }
}


