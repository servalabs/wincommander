$script:HostsFilePath = Join-Path ([Environment]::GetFolderPath('System')) 'drivers\etc\hosts'
$script:HostsMutexName = 'Global\WinCommander_HostsFile'

function Invoke-WithHostsMutex {
    param([scriptblock]$Action)
    $mutex = [Threading.Mutex]::new($false, $script:HostsMutexName)
    $acquired = $false
    try {
        try { $acquired = $mutex.WaitOne(15000) }
        catch [Threading.AbandonedMutexException] { $acquired = $true }
        if (-not $acquired) { throw 'Timed out waiting for hosts file lock.' }
        & $Action
    } finally {
        if ($acquired) { $mutex.ReleaseMutex() }
        $mutex.Dispose()
    }
}

function Get-HostsBlocklistNames {
    return @('telemetry-blocklist', 'ai-sites', 'piracy-torrent', 'glasswire', 'lightburn', 'corel', 'adobe', 'autodesk', 'altium', 'solidworks', 'cloud-upload')
}

function Get-AppliedBlocklistsFromContent {
    param([string]$Content)
    $section = Get-HostsManagedSection $Content
    $match = [regex]::Match($section, '(?m)^# Applied:\s*([^\r\n]+)')
    if ($match.Success) { return @($match.Groups[1].Value -split ',\s*' | Where-Object { $_ }) }
    return @()
}

function New-HostsManagedSection {
    param([string[]]$Names, $Hardcoded)
    if (-not $Names.Count) { return '' }
    $parts = @('# WINCOMMANDER_BLOCKLIST_START', ('# Applied: ' + ($Names -join ', ')), '# Managed by WinCommander - Do not edit manually', '')
    foreach ($name in $Names) {
        if (-not $Hardcoded.ContainsKey($name)) { throw "Unknown installed blocklist '$name'. No changes were made." }
        $parts += "# --- $name ---"
        $parts += $Hardcoded[$name].Trim()
        $parts += ''
    }
    $parts += '# WINCOMMANDER_BLOCKLIST_END'
    return $parts -join "`r`n"
}

function Get-BlocklistStatus {
    try {
        return Invoke-WithHostsMutex {
            $availableBlocklistNames = Get-HostsBlocklistNames
            $hardcoded = Get-HardcodedBlocklists
            $content = Read-HostsContent
            $applied = @(Get-AppliedBlocklistsFromContent $content)
            $managed = Get-HostsRecords (Get-HostsManagedSection $content)
            $all = Get-HostsRecords $content
            $available = @($availableBlocklistNames | ForEach-Object {
                $expected = Get-HostsRecords $hardcoded[$_]
                @{
                    name = $_; entries = $expected.Count
                    verification = Get-HostsListVerification $_ $content $applied $hardcoded $managed $all $expected
                }
            })
            return @{
                available = $available; applied = $applied; checkedAt = [DateTime]::UtcNow.ToString('o')
                undo = Get-HostsUndoStatus $content; telemetryPolicy = Get-TelemetryPolicy
            }
        }
    } catch { return @{ error = $true; message = "Failed to inspect hosts protection: $($_.Exception.Message)" } }
}

function Set-HostsBlocklist {
    param([string[]]$BlocklistNames, [bool]$Enabled, [string]$Label)
    Assert-IsAdmin
    try {
        if (-not $BlocklistNames.Count) { throw 'No blocklist was selected.' }
        foreach ($name in $BlocklistNames) {
            if ($name -notin (Get-HostsBlocklistNames)) { throw 'Unknown blocklist.' }
        }
        return Invoke-WithHostsMutex {
            $hardcoded = Get-HardcodedBlocklists
            $before = Read-HostsContent
            $names = @(Get-AppliedBlocklistsFromContent $before)
            foreach ($name in $BlocklistNames) {
                if ($Enabled -and $names -notcontains $name) { $names += $name }
                if (-not $Enabled) { $names = @($names | Where-Object { $_ -ne $name }) }
            }
            $section = New-HostsManagedSection $names $hardcoded
            $after = Set-HostsManagedSection $before $section
            $changed = Save-HostsChange $before $after $Label
            $actual = Read-HostsContent
            $actualNames = @(Get-AppliedBlocklistsFromContent $actual)
            foreach ($name in $BlocklistNames) {
                $verification = Get-HostsListVerification $name $actual $actualNames $hardcoded
                if (($Enabled -and $verification.state -ne 'verified') -or (-not $Enabled -and $actualNames -contains $name)) {
                    throw 'Protection was only partially applied. Refresh its status before trying again.'
                }
            }
            $dnsWarning = if ($changed) { Clear-HostsDnsCache } else { $null }
            return @{ status = 'success'; verified = $true; changed = $changed; dnsWarning = $dnsWarning; message = 'Hosts protection verified.' }
        }
    } catch { return @{ error = $true; message = $_.Exception.Message } }
}

function Add-BlocklistToHosts {
    param([string]$BlocklistName)
    return Set-HostsBlocklist @($BlocklistName) $true ('Enable ' + $BlocklistName)
}

function Remove-BlocklistFromHosts {
    param([string]$BlocklistName)
    return Set-HostsBlocklist @($BlocklistName) $false ('Disable ' + $BlocklistName)
}

function Set-HostsRecommendedProtection {
    param([bool]$Enabled)
    $names = @(Get-HostsBlocklistNames | Where-Object { $_ -notin @('piracy-torrent', 'cloud-upload', 'ai-sites') })
    return Set-HostsBlocklist $names $Enabled 'Recommended hosts protection'
}
