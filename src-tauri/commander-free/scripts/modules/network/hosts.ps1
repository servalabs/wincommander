# ============================================================================
# NETWORK - HOSTS FILE BLOCKLIST MODULE
# Manages hosts file blocklists for ad/tracker/telemetry blocking
# ============================================================================

$script:HostsFilePath = 'C:\Windows\System32\drivers\etc\hosts'
$script:HostsMutexName = 'Global\WinCommander_HostsFile'

# Helper: Acquire a system-wide named mutex, run a scriptblock, release it.
# Ensures only one process/thread can modify the hosts file at a time.
function Invoke-WithHostsMutex {
    param([scriptblock]$Action)
    $mutex = [System.Threading.Mutex]::new($false, $script:HostsMutexName)
    $acquired = $false
    try {
        # Wait up to 15 seconds for the lock
        $acquired = $mutex.WaitOne(15000)
        if (-not $acquired) { throw "Timed out waiting for hosts file lock" }
        & $Action
    }
    finally {
        if ($acquired) { $mutex.ReleaseMutex() }
        $mutex.Dispose()
    }
}

# Helper: Atomically write content to the hosts file via temp file + rename.
# Prevents partial-write corruption if the process is killed mid-write.
function Write-HostsFileAtomic {
    param([string]$Content)
    $tempPath = "$script:HostsFilePath.wctmp"
    [System.IO.File]::WriteAllText($tempPath, $Content, [System.Text.Encoding]::UTF8)
    Move-Item -Path $tempPath -Destination $script:HostsFilePath -Force
}

# Helper: Get available blocklist names
function Get-HostsBlocklistNames {
    return @('telemetry-blocklist', 'ai-sites', 'piracy-torrent', 'glasswire', 'lightburn', 'corel', 'adobe', 'autodesk', 'cloud-upload')
}

# Helper: Extract applied blocklist names from WINCOMMANDER section
function Get-AppliedBlocklistsFromContent {
    param([string]$Content)
    if ([string]::IsNullOrWhiteSpace($Content)) { return @() }
    
    $lines = $Content -split "`r?`n"
    foreach ($line in $lines) {
        if ($line -match '^# Applied:\s*(.+)$') {
            return $Matches[1] -split ',\s*' | ForEach-Object { $_.Trim() } | Where-Object { $_ }
        }
    }
    return @()
}

# Helper: Remove WINCOMMANDER blocklist section, preserving everything else
function Remove-BlocklistSection {
    param([string]$Content)
    if ([string]::IsNullOrWhiteSpace($Content)) { return "" }
    
    $lines = $Content -split "`r?`n"
    $newLines = @()
    $inSection = $false
    foreach ($line in $lines) {
        if ($line -match '# WINCOMMANDER_BLOCKLIST_START') {
            $inSection = $true
            continue
        }
        if ($line -match '# WINCOMMANDER_BLOCKLIST_END') {
            $inSection = $false
            continue
        }
        if (-not $inSection) {
            $newLines += $line
        }
    }
    return ($newLines -join "`r`n").Trim()
}

# Get current blocklist status for all available blocklists
function Get-BlocklistStatus {
    <#
    .SYNOPSIS
    Returns status of all available blocklists
    #>
    try {
        $availableBlocklistNames = Get-HostsBlocklistNames
        $hardcoded = Get-HardcodedBlocklists
        
        # Read current hosts file
        $hostsContent = ""
        if (Test-Path $script:HostsFilePath) {
            $hostsContent = Get-Content $script:HostsFilePath -Raw -Encoding utf8 -ErrorAction SilentlyContinue
        }
        
        # Determine applied blocklists from hosts file markers
        $appliedBlocklists = @(Get-AppliedBlocklistsFromContent -Content $hostsContent)
        
        # Build available blocklists with metadata
        $available = @()
        foreach ($name in $availableBlocklistNames) {
            $content = $hardcoded[$name]
            # Count entries (lines that start with 0.0.0.0 or 127.0.0.1)
            $count = 0
            if ($content) {
                $count = ($content -split "`r?`n" | Where-Object { $_ -match '^(0\.0\.0\.0|127\.0\.0\.1)' }).Count
            }
            
            $available += @{
                name    = $name
                entries = $count
            }
        }
        
        return @{
            available = @($available)
            applied   = @($appliedBlocklists)
        }
    }
    catch {
        return @{
            error     = $true
            message   = "Failed to get blocklist status: $($_.Exception.Message)"
            available = @()
            applied   = @()
        }
    }
}

# Add a specific blocklist to hosts file
function Add-BlocklistToHosts {
    <#
    .SYNOPSIS
    Adds a blocklist to the Windows hosts file
    #>
    param([string]$BlocklistName)
    
    Assert-IsAdmin
    
    try {
        # Get blocklist content first (outside the lock — read-only)
        $hardcodedBlocklists = Get-HardcodedBlocklists
        if (-not $hardcodedBlocklists.ContainsKey($BlocklistName)) {
            throw "Blocklist '$BlocklistName' not found"
        }

        Invoke-WithHostsMutex {
            # Read current hosts file (inside lock)
            $hostsContent = ""
            if (Test-Path $script:HostsFilePath) {
                $hostsContent = [System.IO.File]::ReadAllText($script:HostsFilePath, [System.Text.Encoding]::UTF8)
            }
            
            # Get currently applied blocklists from hosts file
            $appliedBlocklists = @(Get-AppliedBlocklistsFromContent -Content $hostsContent)
            
            # Add this blocklist to the list if not already there
            if ($appliedBlocklists -notcontains $BlocklistName) {
                $appliedBlocklists += $BlocklistName
            }
            
            # Clean current section (preserves everything else)
            $cleanHostsContent = Remove-BlocklistSection $hostsContent
            
            # Build combined blocklist content
            $combinedContent = ""
            foreach ($name in $appliedBlocklists) {
                if ($hardcodedBlocklists.ContainsKey($name)) {
                    $combinedContent += "# --- $name ---`r`n"
                    $combinedContent += $hardcodedBlocklists[$name] + "`r`n`r`n"
                }
            }
            
            # Build new WINCOMMANDER section
            $wincommanderSection = "`r`n`r`n# WINCOMMANDER_BLOCKLIST_START`r`n# Applied: $($appliedBlocklists -join ', ')`r`n# Managed by WinCommander - Do not edit manually`r`n`r`n$combinedContent# WINCOMMANDER_BLOCKLIST_END"
            
            # Append WINCOMMANDER section to the end of preserved content
            $newHostsContent = $cleanHostsContent.Trim() + $wincommanderSection
            
            # Write atomically (temp file + rename — no partial-write corruption)
            Write-HostsFileAtomic -Content $newHostsContent
        }
        
        # Flush DNS cache (background, outside the lock)
        Start-Job -ScriptBlock { ipconfig /flushdns } | Out-Null
        
        return @{
            status  = 'success'
            message = "Added $BlocklistName to hosts file"
        }
    }
    catch {
        return @{
            error   = $true
            message = "Failed to add blocklist '$BlocklistName': $($_.Exception.Message)"
        }
    }
}

# Remove a specific blocklist from hosts file
function Remove-BlocklistFromHosts {
    <#
    .SYNOPSIS
    Removes a blocklist from the Windows hosts file
    #>
    param([string]$BlocklistName)
    
    Assert-IsAdmin
    
    try {
        # Load hardcoded lists (outside lock — read-only)
        $hardcodedBlocklists = Get-HardcodedBlocklists

        Invoke-WithHostsMutex {
            # Read current hosts file (inside lock)
            $hostsContent = ""
            if (Test-Path $script:HostsFilePath) {
                $hostsContent = [System.IO.File]::ReadAllText($script:HostsFilePath, [System.Text.Encoding]::UTF8)
            }
            
            # Get currently applied blocklists from hosts file
            $appliedBlocklists = @(Get-AppliedBlocklistsFromContent -Content $hostsContent)
            
            # Remove this blocklist from the list
            $appliedBlocklists = $appliedBlocklists | Where-Object { $_ -ne $BlocklistName }
            
            # Clean current section
            $cleanHostsContent = Remove-BlocklistSection $hostsContent
            
            if ($appliedBlocklists.Count -gt 0) {
                # Build combined blocklist content
                $combinedContent = ""
                foreach ($name in $appliedBlocklists) {
                    if ($hardcodedBlocklists.ContainsKey($name)) {
                        $combinedContent += "# --- $name ---`r`n"
                        $combinedContent += $hardcodedBlocklists[$name] + "`r`n`r`n"
                    }
                }
                
                # Build new WINCOMMANDER section
                $wincommanderSection = "`r`n`r`n# WINCOMMANDER_BLOCKLIST_START`r`n# Applied: $($appliedBlocklists -join ', ')`r`n# Managed by WinCommander - Do not edit manually`r`n`r`n$combinedContent# WINCOMMANDER_BLOCKLIST_END"
                
                $newHostsContent = $cleanHostsContent.Trim() + $wincommanderSection
                Write-HostsFileAtomic -Content $newHostsContent
            }
            else {
                # No blocklists remain — write only the preserved (non-WC) content
                Write-HostsFileAtomic -Content $cleanHostsContent.Trim()
            }
        }
        
        # Flush DNS cache (background, outside the lock)
        Start-Job -ScriptBlock { ipconfig /flushdns } | Out-Null
        
        return @{
            status  = 'success'
            message = "Removed $BlocklistName from hosts file"
        }
    }
    catch {
        return @{
            error   = $true
            message = "Failed to remove blocklist '$BlocklistName': $($_.Exception.Message)"
        }
    }
}
