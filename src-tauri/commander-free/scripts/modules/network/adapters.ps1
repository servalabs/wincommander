$Script:WC_NET_CLASS_PATH = 'HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e972-e325-11ce-bfc1-08002be10318}'

function Get-WCAdapterClassKey {
    param([string]$InterfaceGuid)
    Get-ChildItem -LiteralPath $Script:WC_NET_CLASS_PATH -ErrorAction Stop | Where-Object { $_.PSChildName -match '^\d{4}$' } | ForEach-Object {
        $entry = Get-ItemProperty -LiteralPath $_.PSPath -ErrorAction Stop
        if ($entry.NetCfgInstanceId -and ([string]$entry.NetCfgInstanceId).Trim('{}') -ieq $InterfaceGuid.Trim('{}')) { $_.PSPath }
    } | Select-Object -First 1
}

function ConvertTo-WCMac {
    param($Value)
    $mac = ([string]$Value -replace '[:\-]', '').ToUpperInvariant()
    if ($mac -match '^[0-9A-F]{12}$' -and $mac -ne '000000000000') { return $mac }
    return $null
}

function New-WCRandomLocallyAdministeredMac {
    $bytes = New-Object byte[] 6
    $random = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try { $random.GetBytes($bytes) } finally { $random.Dispose() }
    $bytes[0] = ($bytes[0] -band 0xFC) -bor 0x02
    return ($bytes | ForEach-Object { $_.ToString('X2') }) -join ''
}

function Get-PhysicalNetworkAdapters {
    $checkedAt = [DateTime]::UtcNow.ToString('o')
    try {
        $adapters = @(Get-NetAdapter -Physical -ErrorAction Stop | Where-Object { $_.Status -ne 'Not Present' })
        $rows = @($adapters | ForEach-Object {
            $adapter = $_; $configuration = $null; $configurationError = $null
            try {
                $key = Get-WCAdapterClassKey ([string]$adapter.InterfaceGuid)
                if ($key) { $configuration = Get-ItemProperty -LiteralPath $key -ErrorAction Stop }
                else { $configurationError = 'Adapter registry mapping unavailable.' }
            } catch { $configurationError = 'Adapter configuration could not be read.' }
            $factory = ConvertTo-WCMac $adapter.PermanentAddress
            $current = ConvertTo-WCMac $adapter.MacAddress
            $configured = ConvertTo-WCMac $configuration.NetworkAddress
            $kind = 'ethernet'
            if ($adapter.PhysicalMediaType -match 'Wireless|802\.11' -or $adapter.InterfaceType -eq 71) { $kind = 'wifi' }
            elseif ($adapter.PhysicalMediaType -match 'Bluetooth') { $kind = 'bluetooth' }
            $macState = 'unknown'
            if ($factory -and $current) { $macState = if ($factory -eq $current) { 'factory' } else { 'changed' } }
            @{
                id = [string]$adapter.InterfaceGuid; groupId = [string]$adapter.PnPDeviceID
                name = $adapter.Name; description = $adapter.InterfaceDescription; kind = $kind
                status = [string]$adapter.Status; adminStatus = [string]$adapter.AdminStatus
                linkSpeedMbps = if ($adapter.LinkSpeed -and $adapter.LinkSpeed -notmatch '^0\s*bps') { [string]$adapter.LinkSpeed } else { $null }
                factoryMac = $factory; currentMac = $current; isSpoofed = ($macState -eq 'changed'); macState = $macState
                configuredMac = $configured; configuredMode = [string]$configuration.WCMacMode
                configurationError = $configurationError; recoveryPending = [bool]$configuration.WCMacRecovery
                overrideSupport = 'unknown'; checkedAt = $checkedAt
            }
        })
        return @{ status = 'ok'; adapters = $rows; checkedAt = $checkedAt }
    } catch {
        return @{ status = 'unavailable'; adapters = @(); checkedAt = $checkedAt; message = 'Windows adapter inventory could not be read. Refresh or reopen as administrator.' }
    }
}

function Get-WCPhysicalAdapter {
    param([string]$AdapterId)
    $matchingAdapters = @(Get-NetAdapter -Physical -ErrorAction Stop | Where-Object { ([string]$_.InterfaceGuid).Trim('{}') -ieq $AdapterId.Trim('{}') -and $_.Status -ne 'Not Present' })
    if ($matchingAdapters.Count -ne 1) { throw 'The selected physical adapter is no longer available. Refresh the adapter list.' }
    return $matchingAdapters[0]
}

function Test-WCRemoteSession { return ($env:SESSIONNAME -like 'RDP-*') }

function Assert-WCAdapterRestartSafe {
    param($Adapter)
    $addresses = @(Get-NetIPAddress -ErrorAction Stop | Where-Object { $_.InterfaceIndex -eq $Adapter.InterfaceIndex } | ForEach-Object { $_.IPAddress })
    $remote = @(Get-NetTCPConnection -ErrorAction Stop | Where-Object {
        $_.State -eq 'Established' -and $addresses -contains $_.LocalAddress -and $_.LocalPort -in @(22, 3389, 5985, 5986, 7070)
    })
    if ($remote.Count -gt 0 -or (Test-WCRemoteSession)) {
        throw 'This adapter may carry an active remote-management session. Make this change locally after closing remote sessions.'
    }
}

function Set-WCAdapterRegistryValue {
    param([string]$Key, [string]$Name, $Value)
    if ($null -ne $Value) { Set-ItemProperty -LiteralPath $Key -Name $Name -Value ([string]$Value) -Type String -Force -ErrorAction Stop }
    else {
        $properties = Get-ItemProperty -LiteralPath $Key -ErrorAction Stop
        if ($properties.PSObject.Properties.Name -contains $Name) { Remove-ItemProperty -LiteralPath $Key -Name $Name -ErrorAction Stop }
    }
}

function Wait-WCAdapterMac {
    param([string]$AdapterId, $Expected)
    $last = $null
    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        try {
            $last = Get-WCPhysicalAdapter $AdapterId
            if ($Expected -and (ConvertTo-WCMac $last.MacAddress) -eq $Expected) { return $last }
        } catch { }
        if (-not $Expected) { break }
        Start-Sleep -Milliseconds 500
    }
    return $last
}

function Invoke-WCAdapterMacChange {
    param([string]$AdapterId, [string]$Mode, [bool]$RestorePrevious = $false)
    Assert-IsAdmin
    $guid = [Guid]::Empty
    if (-not [Guid]::TryParse($AdapterId, [ref]$guid)) { return @{ status = 'blocked'; message = 'Invalid adapter identity.' } }
    if ($Mode -notin @('off', 'static-random')) { return @{ status = 'blocked'; message = 'Automatic MAC rotation is unavailable. Select a one-time random address.' } }
    $AdapterId = '{' + $guid.ToString() + '}'
    $mutex = $null; $locked = $false; $before = $null; $key = $null; $changed = $false; $restartAttempted = $false
    try {
        $mutex = New-Object System.Threading.Mutex($false, ('Global\WinCommander.Mac.' + $guid.ToString()))
        try { $locked = $mutex.WaitOne(0) } catch [System.Threading.AbandonedMutexException] { $locked = $true }
        if (-not $locked) { return @{ status = 'blocked'; message = 'An operation is already running on this adapter.' } }
        $adapter = Get-WCPhysicalAdapter $AdapterId
        Assert-WCAdapterRestartSafe $adapter
        if ([string]$adapter.AdminStatus -eq 'Down') { return @{ status = 'blocked'; message = 'The adapter is disabled. Enable it locally before changing or restoring its MAC address.' } }
        $key = Get-WCAdapterClassKey $AdapterId
        if (-not $key) { throw 'Adapter registry mapping unavailable.' }
        $properties = Get-ItemProperty -LiteralPath $key -ErrorAction Stop
        $before = @{ networkAddress = $properties.NetworkAddress; mode = $properties.WCMacMode; currentMac = (ConvertTo-WCMac $adapter.MacAddress) }
        $journal = $properties.WCMacRecovery
        if ($journal -and -not $RestorePrevious) { return @{ status = 'blocked'; message = 'An earlier change needs recovery. Use Undo interrupted change first.' } }
        $requested = if ($Mode -eq 'static-random') { New-WCRandomLocallyAdministeredMac } else { $null }
        $requestedMode = if ($Mode -eq 'static-random') { $Mode } else { $null }
        $expected = if ($requested) { $requested } else { ConvertTo-WCMac $adapter.PermanentAddress }
        if ($RestorePrevious) {
            if (-not $journal) { throw 'No interrupted change is available to undo.' }
            $saved = $journal | ConvertFrom-Json -ErrorAction Stop
            if ($saved.adapterId -ne $AdapterId -or $saved.version -ne 1) { throw 'Recovery identity is invalid.' }
            $requested = $saved.networkAddress; $requestedMode = $saved.mode; $expected = ConvertTo-WCMac $saved.currentMac
            if ($requested -and -not (ConvertTo-WCMac $requested)) { throw 'Recovery address is invalid.' }
        } else {
            $record = @{ version = 1; adapterId = $AdapterId; networkAddress = $before.networkAddress; mode = $before.mode; currentMac = $before.currentMac }
            Set-WCAdapterRegistryValue $key 'WCMacRecovery' ($record | ConvertTo-Json -Compress)
        }
        $changed = $true
        Set-WCAdapterRegistryValue $key 'NetworkAddress' $requested
        Set-WCAdapterRegistryValue $key 'WCMacMode' $requestedMode
        $readback = Get-ItemProperty -LiteralPath $key -ErrorAction Stop
        if ([string]$readback.NetworkAddress -ne [string]$requested -or [string]$readback.WCMacMode -ne [string]$requestedMode) { throw 'Adapter settings did not pass read-back.' }
        $adapter = Get-WCPhysicalAdapter $AdapterId
        Assert-WCAdapterRestartSafe $adapter
        if ([string]$adapter.AdminStatus -eq 'Down') {
            return @{ status = 'pending_restart'; message = 'Settings saved. The adapter is disabled; enable it locally and verify the address. Recovery remains available.' }
        }
        $restartAttempted = $true
        Restart-NetAdapter -InputObject $adapter -Confirm:$false -ErrorAction Stop
        $observed = Wait-WCAdapterMac $AdapterId $expected
        $observedMac = ConvertTo-WCMac $observed.MacAddress
        if (-not $expected) {
            $message = if ($RestorePrevious) { 'Previous settings restored. The original address was unknown, so MAC restoration could not be verified. The recovery record was retained.' } else { 'Override removed. Windows did not report a permanent address, so factory restoration could not be verified. Undo interrupted change can restore the previous settings.' }
            return @{ status = 'unverified'; message = $message; observedMac = $observedMac; linkStatus = [string]$observed.Status }
        }
        if ($observedMac -ne $expected) { throw 'Windows did not confirm the requested MAC. The driver may ignore overrides or require more time.' }
        Set-WCAdapterRegistryValue $key 'WCMacRecovery' $null
        return @{ status = 'verified'; observedMac = $observedMac; configuredMac = $requested; linkStatus = [string]$observed.Status; checkedAt = [DateTime]::UtcNow.ToString('o') }
    } catch {
        $failure = $_.Exception.Message
        if (-not $changed) { return @{ status = 'blocked'; message = $failure } }
        try {
            $latest = Get-ItemProperty -LiteralPath $key -ErrorAction Stop
            if ([string]$latest.NetworkAddress -notin @([string]$requested, [string]$before.networkAddress) -or [string]$latest.WCMacMode -notin @([string]$requestedMode, [string]$before.mode)) { throw 'Another actor changed the adapter settings; automatic rollback was stopped.' }
            Set-WCAdapterRegistryValue $key 'NetworkAddress' $before.networkAddress
            Set-WCAdapterRegistryValue $key 'WCMacMode' $before.mode
            $restored = Get-ItemProperty -LiteralPath $key -ErrorAction Stop
            if ([string]$restored.NetworkAddress -ne [string]$before.networkAddress -or [string]$restored.WCMacMode -ne [string]$before.mode) { throw 'Previous settings could not be restored.' }
            $adapter = Get-WCPhysicalAdapter $AdapterId
            Assert-WCAdapterRestartSafe $adapter
            if ([string]$adapter.AdminStatus -eq 'Down') { throw 'Previous settings saved; the adapter remains disabled.' }
            if ($restartAttempted) { Restart-NetAdapter -InputObject $adapter -Confirm:$false -ErrorAction Stop }
            $observed = Wait-WCAdapterMac $AdapterId $before.currentMac
            if (-not $before.currentMac -or (ConvertTo-WCMac $observed.MacAddress) -ne $before.currentMac) { throw 'Previous MAC could not be verified.' }
            if (-not $RestorePrevious) { Set-WCAdapterRegistryValue $key 'WCMacRecovery' $null }
            return @{ status = 'rolled_back'; message = "$failure Previous settings and MAC were restored."; observedMac = (ConvertTo-WCMac $observed.MacAddress); linkStatus = [string]$observed.Status }
        } catch {
            return @{ status = 'failed'; message = "$failure Recovery could not be verified. Use Undo interrupted change locally; the recovery record was retained." }
        }
    } finally {
        if ($locked) { $mutex.ReleaseMutex() }
        if ($mutex) { $mutex.Dispose() }
    }
}

function Set-AdapterRandomMAC {
    param([Parameter(Mandatory = $true)][string]$AdapterId, [Parameter(Mandatory = $true)][string]$Mode)
    Invoke-WCAdapterMacChange -AdapterId $AdapterId -Mode $Mode
}

function Restore-AdapterMAC {
    param([Parameter(Mandatory = $true)][string]$AdapterId, [bool]$RestorePrevious = $false)
    Invoke-WCAdapterMacChange -AdapterId $AdapterId -Mode 'off' -RestorePrevious $RestorePrevious
}
