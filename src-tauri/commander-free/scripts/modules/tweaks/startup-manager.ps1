# ============================================================================
# TWEAKS - STARTUP MANAGER
# Manages Windows startup applications and impact analysis
# ============================================================================

# Knowledge Base for Startup Items
# Action: 'Keep' (Critical/Drivers) or 'Disable' (Bloat/Optional)
$Global:StartupKnowledgeBase = @(
    # --- DRIVERS & HARDWARE (KEEP) ---
    @{ Pattern = "RtkNGUI"; Type = "Driver"; Description = "Realtek Audio Manager"; Action = "Keep" }
    @{ Pattern = "RAVCpl"; Type = "Driver"; Description = "Realtek Audio Control"; Action = "Keep" }
    @{ Pattern = "RtkAudioService"; Type = "Driver"; Description = "Realtek Audio Service"; Action = "Keep" }
    @{ Pattern = "Dolby"; Type = "Driver"; Description = "Dolby Audio"; Action = "Keep" }
    @{ Pattern = "DTS"; Type = "Driver"; Description = "DTS Audio"; Action = "Keep" }
    @{ Pattern = "Syntp"; Type = "Driver"; Description = "Synaptics Touchpad"; Action = "Keep" }
    @{ Pattern = "ETDCtrl"; Type = "Driver"; Description = "ELAN Touchpad"; Action = "Keep" }
    @{ Pattern = "Apoint"; Type = "Driver"; Description = "Alps Touchpad"; Action = "Keep" }
    @{ Pattern = "LogiOptions"; Type = "Driver"; Description = "Logitech Options"; Action = "Keep" }
    @{ Pattern = "SetPoint"; Type = "Driver"; Description = "Logitech SetPoint"; Action = "Keep" }
    @{ Pattern = "igfx"; Type = "Driver"; Description = "Intel Graphics"; Action = "Keep" }
    @{ Pattern = "cnext"; Type = "Driver"; Description = "AMD Radeon Settings"; Action = "Keep" }
    @{ Pattern = "NVIDIA"; Type = "Driver"; Description = "NVIDIA Settings"; Action = "Keep" }
    @{ Pattern = "HP"; Type = "OEM"; Description = "HP Utilities"; Action = "Keep" }
    @{ Pattern = "Dell"; Type = "OEM"; Description = "Dell Utilities"; Action = "Keep" }
    @{ Pattern = "Lenovo"; Type = "OEM"; Description = "Lenovo Utilities"; Action = "Keep" }
    @{ Pattern = "Asus"; Type = "OEM"; Description = "Asus Utilities"; Action = "Keep" }
    @{ Pattern = "Acer"; Type = "OEM"; Description = "Acer Utilities"; Action = "Keep" }

    # --- OPTIONAL / BLOAT (DISABLE BY DEFAULT) ---
    # Cloud Storage (User requested disable)
    @{ Pattern = "OneDrive"; Type = "Cloud"; Description = "Microsoft OneDrive"; Action = "Disable" }
    @{ Pattern = "Dropbox"; Type = "Cloud"; Description = "Dropbox"; Action = "Disable" }
    @{ Pattern = "GoogleDrive"; Type = "Cloud"; Description = "Google Drive"; Action = "Disable" }
    
    # Gaming (User requested disable)
    @{ Pattern = "Steam"; Type = "Gaming"; Description = "Steam Client"; Action = "Disable" }
    @{ Pattern = "EpicGames"; Type = "Gaming"; Description = "Epic Games Launcher"; Action = "Disable" }
    @{ Pattern = "Origin"; Type = "Gaming"; Description = "EA Origin"; Action = "Disable" }
    @{ Pattern = "Battle.net"; Type = "Gaming"; Description = "Battle.net"; Action = "Disable" }
    @{ Pattern = "Discord"; Type = "Communication"; Description = "Discord"; Action = "Disable" }
    
    # VPN
    @{ Pattern = "NordVPN"; Type = "Network"; Description = "NordVPN"; Action = "Disable" }
    @{ Pattern = "ProtonVPN"; Type = "Network"; Description = "ProtonVPN"; Action = "Disable" }

    # System (User requested disable)
    @{ Pattern = "SecurityHealth"; Type = "System"; Description = "Windows Security Notification"; Action = "Disable" }
    @{ Pattern = "Copilot"; Type = "AI"; Description = "Microsoft Copilot"; Action = "Disable" }
    
    # Common Bloat
    @{ Pattern = "Spotify"; Type = "Media"; Description = "Spotify"; Action = "Disable" }
    @{ Pattern = "iTunes"; Type = "Media"; Description = "iTunes Helper"; Action = "Disable" }
    @{ Pattern = "Skype"; Type = "Communication"; Description = "Skype"; Action = "Disable" }
    @{ Pattern = "Teams"; Type = "Communication"; Description = "Microsoft Teams"; Action = "Disable" }
    @{ Pattern = "Cortana"; Type = "System"; Description = "Cortana"; Action = "Disable" }
    @{ Pattern = "Edge"; Type = "Browser"; Description = "Microsoft Edge"; Action = "Disable" }
    @{ Pattern = "Chrome"; Type = "Browser"; Description = "Google Chrome"; Action = "Disable" }
)

function Get-StartupItems {
    <#
    .SYNOPSIS
        Lists all startup items from Registry and Startup Folders.
        Checks StartupApproved keys to verify if user disabled them in Task Manager.
        Calculates RAM impact if the process is currently running.
        Flags items based on KnowledgeBase.
    #>
    $items = @()

    # 1. Registry Run Keys (HKCU/HKLM + WOW6432Node)
    $regPaths = @(
        @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"; Hive = "HKCU"; ApprovedPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" }
        @{ Path = "HKLM:\Software\Microsoft\Windows\CurrentVersion\Run"; Hive = "HKLM"; ApprovedPath = "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" }
        @{ Path = "HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run"; Hive = "HKLM_WOW"; ApprovedPath = "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run32" }
        @{ Path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\RunOnce"; Hive = "HKCU_RunOnce"; ApprovedPath = $null }
        @{ Path = "HKLM:\Software\Microsoft\Windows\CurrentVersion\RunOnce"; Hive = "HKLM_RunOnce"; ApprovedPath = $null }
    )

    foreach ($loc in $regPaths) {
        if (Test-Path $loc.Path) {
            $keys = Get-Item $loc.Path
            foreach ($name in $keys.Property) {
                # Skip default property if empty
                if ([string]::IsNullOrWhiteSpace($name)) { continue }

                $command = $keys.GetValue($name)
                $enabled = $true
                
                # Check StartupApproved for Enable/Disable status
                if ($loc.ApprovedPath -and (Test-Path $loc.ApprovedPath)) {
                    $approvedKey = Get-ItemProperty -Path $loc.ApprovedPath -Name $name -ErrorAction SilentlyContinue
                    if ($approvedKey) {
                        $bytes = $approvedKey.$name
                        # Logic: If first byte is odd (e.g. 03), it's disabled. Even (02) is enabled.
                        if ($bytes -and $bytes.Length -gt 0) {
                            if (($bytes[0] % 2) -ne 0) {
                                $enabled = $false
                            }
                        }
                    }
                }

                $items += [PSCustomObject]@{
                    Name     = $name
                    Command  = $command
                    Location = $loc.Path
                    Source   = "Registry"
                    Hive     = $loc.Hive
                    Enabled  = $enabled
                }
            }
        }
    }

    # 2. Startup Folders
    $folderPaths = @(
        @{ Path = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Startup"; Hive = "User_Startup"; ApprovedPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder" }
        @{ Path = "$env:ProgramData\Microsoft\Windows\Start Menu\Programs\Startup"; Hive = "Common_Startup"; ApprovedPath = "HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder" }
    )

    foreach ($loc in $folderPaths) {
        if (Test-Path $loc.Path) {
            $files = Get-ChildItem -Path $loc.Path -Filter "*.lnk"
            foreach ($file in $files) {
                # Resolve Shortcut
                $sh = New-Object -ComObject WScript.Shell
                $shortcut = $sh.CreateShortcut($file.FullName)
                
                $enabled = $true
                # Check StartupApproved for Folders (Name is the filename)
                if ($loc.ApprovedPath -and (Test-Path $loc.ApprovedPath)) {
                    $approvedKey = Get-ItemProperty -Path $loc.ApprovedPath -Name $file.Name -ErrorAction SilentlyContinue
                    if ($approvedKey) {
                        $bytes = $approvedKey.$($file.Name)
                        if ($bytes -and $bytes.Length -gt 0) {
                            if (($bytes[0] % 2) -ne 0) {
                                $enabled = $false
                            }
                        }
                    }
                }

                $items += [PSCustomObject]@{
                    Name     = $file.BaseName
                    Command  = $shortcut.TargetPath
                    Location = $file.FullName
                    Source   = "Folder"
                    Hive     = $loc.Hive
                    Enabled  = $enabled
                }
            }
        }
    }

    # 2.5 Load Disabled Items from Backup (Custom Disabling)
    $settingsDir = "$env:APPDATA\WinCommander\StartupBackups"
    $backupFile = "$settingsDir\disabled_items.json"
    if (Test-Path $backupFile) {
        try {
            $backups = Get-Content $backupFile | ConvertFrom-Json
            if ($backups) {
                if ($backups -isnot [Array]) { $backups = @($backups) }
                foreach ($b in $backups) {
                    # Avoid duplicates if they somehow exist in both
                    $originalLocation = if ($b.OriginalKey) { $b.OriginalKey } else { $b.OriginalLocation }
                    if (!($items | Where-Object { $_.Name -eq $b.Name -and $_.Location -eq $originalLocation })) {
                        $items += [PSCustomObject]@{
                            Name     = $b.Name
                            Command  = if ($b.Command) { $b.Command } else { $b.OriginalLocation }
                            Location = if ($b.OriginalKey) { $b.OriginalKey } else { $b.OriginalLocation }
                            Source   = $b.Type
                            Hive     = "Backup"
                            Enabled  = $false
                        }
                    }
                }
            }
        }
        catch {}
    }

    # 3. Analyze Impact & Apply KnowledgeBase
    $results = @()
    foreach ($item in $items) {
        $impact = 0
        $status = "Stopped"
        
        if (!$item.Enabled) {
            $status = "Disabled"
        }
        else {
            # Simple fuzzy match for process to get RAM usage
            try {
                # Extract basic filename from Command
                $exeName = ""
                if ($item.Command -match '([^\\]+\.exe)') {
                    $exeName = $Matches[1]
                    $proc = Get-Process -Name ($exeName -replace '.exe', '') -ErrorAction SilentlyContinue
                    # Only mark as Running if process is found
                    if ($proc) {
                        $impact = ($proc | Measure-Object -Property WorkingSet -Sum).Sum / 1MB
                        $status = "Running"
                    }
                }
            }
            catch {}
        }
       
        # KnowledgeBase Lookup
        $recommendation = "Neutral" # Default for unknown items
        $category = "Unknown"
        $description = "Unknown Application"

        foreach ($entry in $Global:StartupKnowledgeBase) {
            if ($item.Command -match $entry.Pattern -or $item.Name -match $entry.Pattern) {
                $category = $entry.Type
                $description = $entry.Description
                $recommendation = $entry.Action
                break
            }
        }

        $results += [PSCustomObject]@{
            Name           = $item.Name
            Command        = $item.Command
            RamUsageMB     = [Math]::Round($impact, 2)
            Status         = $status
            IsEnabled      = $item.Enabled
            Source         = $item.Source
            Location       = $item.Location
            Recommendation = $recommendation 
            Category       = $category
            Description    = $description
        }
    }

    return $results
}

function Resolve-WcStartupTarget {
    param([string]$Name, [string]$Location)
    if ([string]::IsNullOrWhiteSpace($Name) -or $Name.IndexOfAny([char[]]'*?[]') -ge 0) { throw 'Select an exact startup entry.' }
    $matches = @(Get-StartupItems | Where-Object { $_.Name -eq $Name -and (-not $Location -or $_.Location -eq $Location) })
    if ($matches.Count -ne 1) { throw 'Startup entry changed or is ambiguous. Scan again and select its exact location.' }
    $item = $matches[0]
    $approvedMap = @{
        'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'
        'HKLM:\Software\Microsoft\Windows\CurrentVersion\Run' = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'
        'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run' = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run32'
    }
    $approved = $approvedMap[$item.Location]
    $valueName = $Name
    if (-not $approved -and $item.Location -notin @('HKCU:\Software\Microsoft\Windows\CurrentVersion\RunOnce', 'HKLM:\Software\Microsoft\Windows\CurrentVersion\RunOnce')) {
        $full = [IO.Path]::GetFullPath($item.Location)
        $parent = [IO.Path]::GetDirectoryName($full)
        $userRoot = [IO.Path]::Combine($env:APPDATA, 'Microsoft\Windows\Start Menu\Programs\Startup')
        $commonRoot = [IO.Path]::Combine($env:ProgramData, 'Microsoft\Windows\Start Menu\Programs\Startup')
        if ($parent -notin @($userRoot, $commonRoot) -or [IO.Path]::GetExtension($full) -ne '.lnk') { throw 'This startup location is not supported.' }
        # The shortcut and each existing ancestor must be ordinary local entries.
        foreach ($candidate in @($full, "$full.disabled", $parent)) {
            if (Test-Path -LiteralPath $candidate) {
                $node = Get-Item -LiteralPath $candidate -Force -ErrorAction Stop
                while ($node) {
                    if (($node.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Linked startup locations cannot be changed here.' }
                    $node = if ($node -is [IO.DirectoryInfo] -or $node.PSIsContainer) { $node.Parent } else { $node.Directory }
                }
            }
        }
        $approved = if ($parent -eq $userRoot) { 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder' } else { 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder' }
        $valueName = [IO.Path]::GetFileName($full)
    }
    return @{ Item = $item; ApprovedPath = $approved; ValueName = $valueName }
}

function Disable-StartupItem {
    param(
        [string]$Name,
        [string]$Location
    )
    Assert-IsAdmin
    try { $target = Resolve-WcStartupTarget -Name $Name -Location $Location }
    catch { return @{ success = $false; message = $_.Exception.Message } }
    $Location = $target.Item.Location

    # 1. Try "Soft Disable" via StartupApproved (Task Manager method) - PREFERRED
    # This keeps the item in the list but stops it from running.
    
    $approvedPath = $target.ApprovedPath

    if ($approvedPath) {
        # Determine the Registry Value Name
        # For Run keys, it's just $Name. For Folders, it's "filename.lnk".
        $regName = $target.ValueName

        # Check if Approved Key exists, create if not
        if (!(Test-Path $approvedPath)) {
            New-Item -Path $approvedPath -Force | Out-Null
        }

        # Get current value or create new
        $currentVal = Get-ItemProperty -Path $approvedPath -Name $regName -ErrorAction SilentlyContinue
        $bytes = @(0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00) # Default disabled struct

        if ($currentVal) {
            $existingBytes = $currentVal.$regName
            if ($existingBytes -and $existingBytes.Length -gt 0) {
                $bytes = $existingBytes
                # Flip first byte to odd (03 = Disabled by User)
                if (($bytes[0] % 2) -eq 0) {
                    $bytes[0] = $bytes[0] + 1
                }
            }
        }
        else {
            # If creating new, we should add a timestamp to be safe (though 0s often work)
            $time = [DateTime]::Now.ToFileTime()
            $timeBytes = [BitConverter]::GetBytes($time)
            # Structure: 03 00 00 00 [8 bytes timestamp]
            for ($i = 0; $i -lt 8; $i++) {
                $bytes[4 + $i] = $timeBytes[$i]
            }
        }

        try {
            Set-ItemProperty -LiteralPath $approvedPath -Name $regName -Value $bytes -Type Binary -Force -ErrorAction Stop
            return @{ success = $true; message = "Disabled via Registry (Soft)" }
        }
        catch {
            return @{ success = $false; message = "Windows could not change this startup entry. No fallback removal was attempted: $($_.Exception.Message)" }
        }
    }

    # 2. Fallback: Hard Disable (Backup & Remove/Rename)
    # Only used if Soft Disable fails or location isn't standard
    
    $settingsDir = "$env:APPDATA\WinCommander\StartupBackups"
    if (!(Test-Path $settingsDir)) { New-Item -Path $settingsDir -ItemType Directory -Force | Out-Null }
    $backupFile = "$settingsDir\disabled_items.json"
    
    $backups = @()
    if (Test-Path $backupFile) {
        $backups = Get-Content $backupFile | ConvertFrom-Json
        if ($backups -isnot [Array]) { $backups = @($backups) }
    }

    if ($Location -match "Startup") {
        # File
        if (Test-Path $Location) {
            $item = [PSCustomObject]@{
                Name             = $Name
                OriginalLocation = $Location
                Type             = "File"
                Timestamp        = (Get-Date).ToString()
            }
            Rename-Item -Path $Location -NewName "$($Name).lnk.disabled" -Force
            $backups += $item
            $backups | ConvertTo-Json -Depth 5 | Set-Content $backupFile
            return @{ success = $true; message = "Disabled via Rename (Hard)" }
        }
    }
    else {
        # Registry Value
        $value = Get-ItemProperty -Path $Location -Name $Name -ErrorAction SilentlyContinue
        if ($value) {
            $command = $value.$Name
            $item = [PSCustomObject]@{
                Name        = $Name
                Command     = $command
                OriginalKey = $Location
                Type        = "Registry"
                Timestamp   = (Get-Date).ToString()
            }
            Remove-ItemSecure -Path $Location -Name $Name -Force
            $backups += $item
            $backups | ConvertTo-Json -Depth 5 | Set-Content $backupFile
            return @{ success = $true; message = "Disabled via Removal (Hard)" }
        }
    }
    
    return @{ success = $false; message = "Failed to disable item" }
}

function Enable-StartupItem {
    param([string]$Name, [string]$Location)
    Assert-IsAdmin
    try {
        $target = Resolve-WcStartupTarget -Name $Name -Location $Location
        $Location = $target.Item.Location
        $backupFile = "$env:APPDATA\WinCommander\StartupBackups\disabled_items.json"
        if (Test-Path -LiteralPath $backupFile) {
            $backups = Get-Content -LiteralPath $backupFile -Raw | ConvertFrom-Json
            $backups = @($backups)
            $matching = @($backups | Where-Object { $_.Name -eq $Name -and ($_.OriginalKey -eq $Location -or $_.OriginalLocation -eq $Location) })
            if ($matching.Count -gt 1) { throw 'Startup backup is ambiguous; no changes were made.' }
            if ($matching.Count -eq 1) {
                $backup = $matching[0]
                if ($backup.Type -eq 'File') {
                    $disabledPath = "$($backup.OriginalLocation).disabled"
                    if (-not (Test-Path -LiteralPath $disabledPath) -or (Test-Path -LiteralPath $backup.OriginalLocation)) { throw 'Startup shortcut changed; backup was kept.' }
                    Rename-Item -LiteralPath $disabledPath -NewName ([IO.Path]::GetFileName($backup.OriginalLocation)) -ErrorAction Stop
                } elseif ($backup.Type -eq 'Registry') {
                    # Never overwrite a newly registered launcher with stale backup content.
                    $existing = Get-ItemProperty -LiteralPath $Location -Name $Name -ErrorAction SilentlyContinue
                    if ($existing) { throw 'Startup entry already exists; backup was kept.' }
                    New-ItemProperty -LiteralPath $Location -Name $Name -Value $backup.Command -PropertyType String -ErrorAction Stop | Out-Null
                } else { throw 'Unsupported startup backup type.' }
                $remaining = @($backups | Where-Object { $_ -ne $backup })
                ConvertTo-Json -InputObject $remaining -Depth 5 | Set-Content -LiteralPath $backupFile
                return @{ success = $true; message = 'Restored the selected startup entry.' }
            }
        }
        if (-not $target.ApprovedPath) { throw 'No disabled backup exists for this entry.' }
        if (-not (Test-Path -LiteralPath $target.ApprovedPath)) { New-Item -Path $target.ApprovedPath -Force -ErrorAction Stop | Out-Null }
        $value = Get-ItemProperty -LiteralPath $target.ApprovedPath -Name $target.ValueName -ErrorAction SilentlyContinue
        $bytes = [byte[]]@(0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
        if ($value -and $value.($target.ValueName) -is [byte[]] -and $value.($target.ValueName).Length -ge 12) { $bytes = $value.($target.ValueName); $bytes[0] = 0x02 }
        Set-ItemProperty -LiteralPath $target.ApprovedPath -Name $target.ValueName -Value $bytes -Type Binary -Force -ErrorAction Stop
        return @{ success = $true; message = 'Enabled the selected startup entry.' }
    } catch { return @{ success = $false; message = $_.Exception.Message } }
}

function Invoke-OptimizeStartup {
    <#
    .SYNOPSIS
        Auto-optimizes startup by disabling safe-to-remove items.
        (Interactive/Guided mode is preferred, this is the 'Auto' logic)
    #>
    param(
        [switch]$Aggressive
    )
    
    $items = Get-StartupItems
    $stats = @{
        DisabledCount = 0
        RamSavedMB    = 0
    }
    
    foreach ($item in $items) {
        if ($item.Recommendation -eq "Keep") { continue }
        
        # If aggressive, we disable everything not recommended to Keep
        # Otherwise, we might want a 'Blacklist' of known bloat
        # For now, we only implement the mechanism, not the policy logic.
        # Implementation depends on UI selection.
        if ($Aggressive -or ($item.Recommendation -eq "Disable")) {
            try {
                Disable-StartupItem -Name $item.Name -Location $item.Location | Out-Null
                $stats.DisabledCount++
                $stats.RamSavedMB += $item.RamUsageMB
            }
            catch {
                Write-Warning "Failed to disable $($item.Name): $($_.Exception.Message)"
            }
        }
    }
    return $stats
}
