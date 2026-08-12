# ============================================================================
# NETWORK - ADAPTERS MODULE
# Per-adapter MAC randomization (Wi-Fi + Ethernet).
#
# Model:
#   - "off"              : factory MAC (default). Spoof reg value cleared.
#   - "static-random"    : one-time random MAC, persisted in registry
#                          (HKLM\SYSTEM\CurrentControlSet\Control\Class\{...}\NNNN\NetworkAddress).
#                          Survives reboots; survives app uninstall until
#                          adapter driver is reinstalled.
#   - "rotate-on-launch" : same as static-random but the WinCommander shell
#                          re-randomizes the MAC every time the app starts.
#                          Mode is recorded so the Rust startup hook can pick
#                          it up — no per-connect WMI watcher required.
#
# Net class GUID is fixed across Windows: {4d36e972-e325-11ce-bfc1-08002be10318}
#
# KT: A single physical card commonly exposes 4-5 NetAdapter instances at
# once — every historical SSID rebinding leaves a "ghost" instance behind
# (Wi-Fi, Wi-Fi 2, Wi-Fi 3 ...). They all share the same PnPDeviceID, so
# PnPDeviceID is NOT a usable unique key. We use InterfaceGuid as `id` (which
# is unique per binding) and surface PnPDeviceID separately as `groupId` so
# the UI can group ghosts under the active adapter without conflating them.
# ============================================================================

$Script:WC_NET_CLASS_GUID = '{4d36e972-e325-11ce-bfc1-08002be10318}'
$Script:WC_NET_CLASS_PATH = "HKLM:\SYSTEM\CurrentControlSet\Control\Class\$($Script:WC_NET_CLASS_GUID)"

function Get-WCAdapterClassKey {
    param([string]$InterfaceGuid)
    if (-not $InterfaceGuid) { return $null }

    Get-ChildItem -Path $Script:WC_NET_CLASS_PATH -ErrorAction SilentlyContinue | ForEach-Object {
        try {
            $val = (Get-ItemProperty -Path $_.PSPath -Name 'NetCfgInstanceId' -ErrorAction Stop).NetCfgInstanceId
            if ($val -and ($val -ieq $InterfaceGuid)) { return $_.PSPath }
        } catch { }
    } | Select-Object -First 1
}

function New-WCRandomLocallyAdministeredMac {
    # Locally-administered, unicast MAC (avoid OUIs that collide with vendors).
    # First octet must have bit1=1 (locally administered) and bit0=0 (unicast).
    $bytes = New-Object byte[] 6
    [System.Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
    $bytes[0] = ($bytes[0] -band 0xFC) -bor 0x02
    return ($bytes | ForEach-Object { $_.ToString('X2') }) -join ''
}

function Get-PhysicalNetworkAdapters {
    Assert-IsAdmin
    $list = New-Object System.Collections.Generic.List[hashtable]

    try {
        # Drop fully-detached ghosts (driver remembers them but hardware is
        # gone). Keep Disconnected/Disabled — those are valid past bindings the
        # UI will tuck under a dropdown so the user can clean them up if they
        # want.
        $rawAdapters = @(
            Get-NetAdapter -Physical -ErrorAction SilentlyContinue |
                Where-Object { $_.Status -ne 'Not Present' }
        )

        foreach ($a in $rawAdapters) {
            $factory = if ($a.PermanentAddress) { ($a.PermanentAddress -replace '[:\-]', '').ToUpper() } else { $null }
            $current = if ($a.MacAddress)       { ($a.MacAddress       -replace '[:\-]', '').ToUpper() } else { $null }
            $isSpoofed = $factory -and $current -and ($factory -ne $current)

            $kind = 'ethernet'
            if ($a.PhysicalMediaType -match 'Wireless|802\.11') { $kind = 'wifi' }
            elseif ($a.InterfaceType -eq 71)                    { $kind = 'wifi' }
            elseif ($a.PhysicalMediaType -match 'Bluetooth')    { $kind = 'bluetooth' }

            # Suppress the bogus "0 bps" Get-NetAdapter reports for cards that
            # are present but not currently linked.
            $linkSpeed = if ($a.LinkSpeed -and $a.LinkSpeed -notmatch '^0\s*bps') { [string]$a.LinkSpeed } else { $null }

            $list.Add(@{
                id              = [string]$a.InterfaceGuid     # unique per instance
                groupId         = [string]$a.PnPDeviceID       # shared across ghost instances
                name            = $a.Name
                description     = $a.InterfaceDescription
                kind            = $kind
                status          = [string]$a.Status
                linkSpeedMbps   = $linkSpeed
                factoryMac      = $factory
                currentMac      = $current
                isSpoofed       = [bool]$isSpoofed
            })
        }
    } catch { }

    return @{ status = 'ok'; adapters = @($list) }
}

function Set-AdapterRandomMAC {
    [CmdletBinding()]
    param(
        # InterfaceGuid, returned by Get-PhysicalNetworkAdapters as `id`.
        [Parameter(Mandatory = $true)]
        [string]$AdapterId,

        # "static-random" | "rotate-on-launch"
        # Rotation hint is stored in the same registry tree under a private
        # value name so it round-trips without us owning a separate config file.
        [Parameter(Mandatory = $true)]
        [ValidateSet('static-random', 'rotate-on-launch')]
        [string]$Mode
    )
    Assert-IsAdmin

    $classKey = Get-WCAdapterClassKey -InterfaceGuid $AdapterId
    if (-not $classKey) { return @{ error = $true; message = "Adapter class key not found for $AdapterId" } }

    $newMac = New-WCRandomLocallyAdministeredMac

    try {
        Set-ItemProperty -Path $classKey -Name 'NetworkAddress' -Value $newMac -Type String -Force -ErrorAction Stop
        # Private hint so Get-PhysicalNetworkAdapters / startup hook can recover the mode.
        Set-ItemProperty -Path $classKey -Name 'WCMacMode' -Value $Mode -Type String -Force -ErrorAction Stop
    } catch {
        return @{ error = $true; message = "Failed to write registry: $($_.Exception.Message)" }
    }

    # Restart the adapter so the new MAC takes effect. Match by InterfaceGuid
    # (unique) instead of PnPDeviceID (shared across ghosts).
    try {
        $adapter = Get-NetAdapter -ErrorAction SilentlyContinue | Where-Object { [string]$_.InterfaceGuid -ieq $AdapterId } | Select-Object -First 1
        if ($adapter) {
            Restart-NetAdapter -Name $adapter.Name -Confirm:$false -ErrorAction Stop
        }
    } catch {
        # Adapter sometimes returns transient errors mid-restart. The MAC value
        # is already in the registry — next link-down/up will apply it.
        return @{ status = 'partial'; appliedMac = $newMac; mode = $Mode; warning = $_.Exception.Message }
    }

    return @{ status = 'ok'; appliedMac = $newMac; mode = $Mode }
}

function Restore-AdapterMAC {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$AdapterId
    )
    Assert-IsAdmin

    $classKey = Get-WCAdapterClassKey -InterfaceGuid $AdapterId
    if (-not $classKey) { return @{ error = $true; message = "Adapter class key not found for $AdapterId" } }

    try {
        Remove-ItemProperty -Path $classKey -Name 'NetworkAddress' -ErrorAction SilentlyContinue
        Remove-ItemProperty -Path $classKey -Name 'WCMacMode' -ErrorAction SilentlyContinue
    } catch {
        return @{ error = $true; message = "Failed to clear registry: $($_.Exception.Message)" }
    }

    try {
        $adapter = Get-NetAdapter -ErrorAction SilentlyContinue | Where-Object { [string]$_.InterfaceGuid -ieq $AdapterId } | Select-Object -First 1
        if ($adapter) { Restart-NetAdapter -Name $adapter.Name -Confirm:$false -ErrorAction Stop }
    } catch {
        return @{ status = 'partial'; warning = $_.Exception.Message }
    }

    return @{ status = 'ok' }
}
