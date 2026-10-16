function Get-TelemetryPolicy {
    return @{
        revision = '2026-10-16'; reviewedAt = '2026-10-16'; reviewType = 'Endpoint scope review'
        summary = 'Blocks selected Windows, app analytics and crash-reporting destinations. Update, security and connectivity endpoints are excluded.'
        sources = @(
            'https://learn.microsoft.com/en-us/windows/privacy/manage-windows-11-endpoints',
            'https://learn.microsoft.com/en-us/defender-endpoint/standard-device-connectivity-urls-commercial'
        )
    }
}

function Get-HostsEssentialOverrides {
    param($Records)
    $essentialPattern = '(^|\.)(events\.data\.microsoft\.com|delivery\.mp\.microsoft\.com|windowsupdate\.com|update\.microsoft\.com|wdcp\.microsoft\.com|wd\.microsoft\.com|endpoint\.security\.microsoft\.com|msftconnecttest\.com|msftncsi\.com)$|^winatp-gw-[^.]+\.microsoft\.com$|^(settings-win\.data|officeclient|dmd\.metaservices)\.microsoft\.com$'
    return @($Records.Keys | Where-Object { $_ -match $essentialPattern } | Sort-Object)
}

function Get-HostsRecords {
    param([string]$Content)
    $records = @{}
    foreach ($line in ($Content -split '\r?\n')) {
        $parts = @(($line -split '#', 2)[0].Trim() -split '\s+')
        if ($parts.Count -lt 2) { continue }
        $address = $null
        if (-not [Net.IPAddress]::TryParse($parts[0], [ref]$address)) { continue }
        foreach ($domain in $parts[1..($parts.Count - 1)]) {
            $key = $domain.TrimEnd('.').ToLowerInvariant()
            if (-not $records.ContainsKey($key)) { $records[$key] = @() }
            $records[$key] += $address.ToString()
        }
    }
    return $records
}

function Test-HostsBlockedRecord {
    param($Records, [string]$Domain)
    if (-not $Records.ContainsKey($Domain)) { return $false }
    foreach ($address in $Records[$Domain]) {
        if ($address -notin @('0.0.0.0', '127.0.0.1', '::', '::1')) { return $false }
    }
    return $true
}

function Get-HostsListVerification {
    param([string]$Name, [string]$Content, [string[]]$Applied, $Hardcoded, $ManagedRecords, $AllRecords, $ExpectedRecords)
    $section = Get-HostsManagedSection $Content
    $managed = if ($null -ne $ManagedRecords) { $ManagedRecords } else { Get-HostsRecords $section }
    $allRecords = if ($null -ne $AllRecords) { $AllRecords } else { Get-HostsRecords $Content }
    $expected = if ($null -ne $ExpectedRecords) { $ExpectedRecords } else { Get-HostsRecords $Hardcoded[$Name] }
    $matched = 0
    foreach ($domain in $expected.Keys) {
        if ((Test-HostsBlockedRecord $managed $domain) -and (Test-HostsBlockedRecord $allRecords $domain)) { $matched++ }
    }
    $state = 'off'
    $message = 'Not enabled.'
    if ($Applied -contains $Name) {
        $state = if ($matched -eq $expected.Count -and $expected.Count -gt 0) { 'verified' } else { 'partial' }
        $message = if ($state -eq 'verified') { 'Applied and verified in the hosts file.' } else { 'Some expected entries are missing or overridden.' }
        if ($Name -eq 'telemetry-blocklist') {
            $legacy = [regex]::Match($section, '(?ms)^# --- telemetry-blocklist ---\r?\n(.*?)(?=^# --- |^# WINCOMMANDER_BLOCKLIST_END)')
            $actual = Get-HostsRecords $legacy.Groups[1].Value
            $extra = @($actual.Keys | Where-Object { -not $expected.ContainsKey($_) })
            $essential = @(Get-HostsEssentialOverrides $managed)
            if ($extra.Count -gt 0 -or $essential.Count -gt 0) {
                $state = 'attention'; $message = 'The installed entries differ from the reviewed list. Update protection to apply the current defaults.'
            }
        }
    }
    return @{ state = $state; message = $message; matched = $matched; expected = $expected.Count }
}

function Clear-HostsDnsCache {
    try { Clear-DnsClientCache -ErrorAction Stop; return $null }
    catch { return 'Entries were verified, but the DNS cache could not be cleared. Existing connections may continue until restarted.' }
}
