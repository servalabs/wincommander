$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/../../src-tauri/commander-free/scripts/modules/network/adapters.ps1"

function Assert-IsAdmin { if ($script:denyAdmin) { throw 'Administrator required' } }
function Test-WCRemoteSession { $false }
function Start-Sleep { param($Milliseconds) }
function Get-WCAdapterClassKey { param($InterfaceGuid) 'TestRegistry' }
function Get-NetAdapter { param([switch]$Physical, $ErrorAction) if ($script:enumerationFails) { throw 'provider unavailable' }; $script:adapter }
function Get-NetIPAddress { param($InterfaceIndex, $ErrorAction) @([pscustomobject]@{ InterfaceIndex = 8; IPAddress = '192.0.2.1' }) }
function Get-NetTCPConnection { param($State, $ErrorAction) if ($script:remote) { [pscustomobject]@{ State = 'Established'; LocalAddress = '192.0.2.1'; LocalPort = 3389 } } }
function Get-ItemProperty { param($LiteralPath, $Path, $ErrorAction) [pscustomobject]$script:registry }
function Set-ItemProperty {
    param($LiteralPath, $Path, $Name, $Value, $Type, [switch]$Force, $ErrorAction)
    if ($Name -eq 'WCMacMode' -and $script:writeFails) { throw 'write denied' }
    if ($Name -eq 'WCMacRecovery' -and $script:journalFails) { throw 'journal denied' }
    $script:registry[$Name] = $Value
}
function Remove-ItemProperty {
    param($LiteralPath, $Path, $Name, $ErrorAction)
    if ($Name -eq 'NetworkAddress' -and $script:removeFails) { throw 'remove denied' }
    $script:registry.Remove($Name)
}
function Restart-NetAdapter {
    param($InputObject, $Name, $Confirm, $ErrorAction)
    $script:restarts++
    if ($script:externalChange) { $script:registry.NetworkAddress = '02AABBCCDDEE'; throw 'external update' }
    if ($script:restartFails) { throw 'restart rejected' }
    if (-not $script:ignoresOverride) {
        $script:adapter.MacAddress = if ($script:registry.ContainsKey('NetworkAddress')) { $script:registry.NetworkAddress } else { $script:adapter.PermanentAddress }
    }
}
function Reset-TestAdapter {
    $script:adapter = [pscustomobject]@{ InterfaceGuid = '{11111111-1111-1111-1111-111111111111}'; PnPDeviceID = 'TEST'; Name = 'Test'; InterfaceDescription = 'Test'; InterfaceIndex = 8; Status = 'Up'; AdminStatus = 'Up'; PermanentAddress = '001122334455'; MacAddress = '001122334455'; PhysicalMediaType = '802.3'; InterfaceType = 6; LinkSpeed = '1 Gbps' }
    $script:registry = @{}; $script:restarts = 0
    $script:enumerationFails = $false; $script:remote = $false; $script:writeFails = $false
    $script:removeFails = $false; $script:restartFails = $false; $script:ignoresOverride = $false
    $script:journalFails = $false; $script:externalChange = $false; $script:denyAdmin = $false
}
function Assert-Equal($Actual, $Expected, $Message) { if ($Actual -ne $Expected) { throw "$Message (expected $Expected, got $Actual)" } }

Reset-TestAdapter
$result = Set-AdapterRandomMAC -AdapterId $script:adapter.InterfaceGuid -Mode 'static-random'
Assert-Equal $result.status 'verified' 'Read-back is required for success'
Assert-Equal $result.observedMac $script:registry.NetworkAddress 'Observed MAC matches registry request'
$result = Restore-AdapterMAC -AdapterId $script:adapter.InterfaceGuid
Assert-Equal $result.status 'verified' 'Factory restore verified'
Assert-Equal $script:adapter.MacAddress '001122334455' 'Factory address restored'

Reset-TestAdapter; $script:ignoresOverride = $true
$result = Set-AdapterRandomMAC -AdapterId $script:adapter.InterfaceGuid -Mode 'static-random'
Assert-Equal $result.status 'rolled_back' 'Ignored override does not report success'
Assert-Equal $script:registry.ContainsKey('NetworkAddress') $false 'Previous registry state restored'

Reset-TestAdapter; $script:remote = $true
$result = Set-AdapterRandomMAC -AdapterId $script:adapter.InterfaceGuid -Mode 'static-random'
Assert-Equal $result.status 'blocked' 'Active remote route blocked'
Assert-Equal $script:registry.Count 0 'No writes to remote adapter'
Assert-Equal $script:restarts 0 'No remote adapter restart'

Reset-TestAdapter; $script:writeFails = $true
$result = Set-AdapterRandomMAC -AdapterId $script:adapter.InterfaceGuid -Mode 'static-random'
Assert-Equal $result.status 'rolled_back' 'Partial write rolls back'
Assert-Equal $script:registry.ContainsKey('NetworkAddress') $false 'Partial write removed'

Reset-TestAdapter; $script:registry.NetworkAddress = '021122334455'; $script:adapter.MacAddress = '021122334455'; $script:removeFails = $true
$result = Restore-AdapterMAC -AdapterId $script:adapter.InterfaceGuid
Assert-Equal ($result.status -eq 'verified') $false 'Registry removal failure cannot succeed'

Reset-TestAdapter; $script:enumerationFails = $true
$result = Get-PhysicalNetworkAdapters
Assert-Equal $result.status 'unavailable' 'Enumeration failure is not empty success'

Reset-TestAdapter; $script:adapter.PermanentAddress = $null
$result = Restore-AdapterMAC -AdapterId $script:adapter.InterfaceGuid
Assert-Equal $result.status 'unverified' 'Unknown permanent address cannot prove factory restore'
Assert-Equal $script:registry.ContainsKey('WCMacRecovery') $true 'Unverified restore retains recovery record'

Reset-TestAdapter; $script:adapter.PermanentAddress = $null
$script:adapter.MacAddress = '021122334466'; $script:registry.NetworkAddress = '021122334466'; $script:registry.WCMacMode = 'static-random'
$result = Restore-AdapterMAC -AdapterId $script:adapter.InterfaceGuid
Assert-Equal $result.status 'unverified' 'Unknown factory address does not pretend restoration succeeded'
Assert-Equal $script:registry.ContainsKey('WCMacRecovery') $true 'Previous custom MAC remains recoverable'
$result = Restore-AdapterMAC -AdapterId $script:adapter.InterfaceGuid -RestorePrevious $true
Assert-Equal $result.status 'verified' 'Explicit recovery verifies previous custom MAC without a permanent address'
Assert-Equal $script:adapter.MacAddress '021122334466' 'Previous custom MAC restored'
Assert-Equal $script:registry.ContainsKey('WCMacRecovery') $false 'Verified recovery clears journal'

Reset-TestAdapter
$result = Set-AdapterRandomMAC -AdapterId $script:adapter.InterfaceGuid -Mode 'rotate-on-launch'
Assert-Equal $result.status 'blocked' 'Unimplemented rotation cannot be enabled'
Assert-Equal $script:registry.Count 0 'Rotation rejection is nonmutating'

Reset-TestAdapter; $script:adapter.InterfaceGuid = '11111111-1111-1111-1111-111111111111'
$result = Set-AdapterRandomMAC -AdapterId $script:adapter.InterfaceGuid -Mode 'static-random'
Assert-Equal $result.status 'verified' 'GUID with or without braces selects the same adapter'
Assert-Equal (([Convert]::ToInt32($result.observedMac.Substring(0, 2), 16)) -band 3) 2 'Random address is unicast and locally administered'

Reset-TestAdapter; $script:journalFails = $true
$result = Set-AdapterRandomMAC -AdapterId $script:adapter.InterfaceGuid -Mode 'static-random'
Assert-Equal $result.status 'blocked' 'Failed recovery record prevents mutation'
Assert-Equal $script:registry.Count 0 'No configuration changed without recovery record'
Assert-Equal $script:restarts 0 'No restart after journal failure'

Reset-TestAdapter; $script:restartFails = $true
$result = Set-AdapterRandomMAC -AdapterId $script:adapter.InterfaceGuid -Mode 'static-random'
Assert-Equal $result.status 'failed' 'Failed restart and rollback are explicit'
Assert-Equal $script:registry.ContainsKey('WCMacRecovery') $true 'Failed rollback retains recovery record'
$script:restartFails = $false
$result = Restore-AdapterMAC -AdapterId $script:adapter.InterfaceGuid -RestorePrevious $true
Assert-Equal $result.status 'verified' 'Interrupted operation can be recovered'
Assert-Equal $script:registry.ContainsKey('WCMacRecovery') $false 'Verified recovery clears journal'

Reset-TestAdapter; $script:registry.NetworkAddress = '021122334466'; $script:registry.WCMacMode = 'static-random'; $script:adapter.MacAddress = '021122334466'; $script:ignoresOverride = $true
$result = Set-AdapterRandomMAC -AdapterId $script:adapter.InterfaceGuid -Mode 'static-random'
Assert-Equal $result.status 'rolled_back' 'Rollback restores previous custom address'
Assert-Equal $script:registry.NetworkAddress '021122334466' 'Rollback does not substitute factory for previous custom address'

Reset-TestAdapter; $script:externalChange = $true
$result = Set-AdapterRandomMAC -AdapterId $script:adapter.InterfaceGuid -Mode 'static-random'
Assert-Equal $result.status 'failed' 'Competing external write stops automatic rollback'
Assert-Equal $script:registry.NetworkAddress '02AABBCCDDEE' 'Another actor configuration is not overwritten'

Reset-TestAdapter; $script:adapter.AdminStatus = 'Down'; $script:adapter.Status = 'Disabled'
$result = Restore-AdapterMAC -AdapterId $script:adapter.InterfaceGuid
Assert-Equal $result.status 'blocked' 'Disabled adapter must be enabled locally first'
Assert-Equal $script:registry.Count 0 'Disabled adapter unchanged'
Assert-Equal $script:restarts 0 'Disabled adapter never implicitly enabled'

Reset-TestAdapter; $script:adapter.Status = 'Disconnected'
$result = Set-AdapterRandomMAC -AdapterId $script:adapter.InterfaceGuid -Mode 'static-random'
Assert-Equal $result.status 'verified' 'Address can be verified independently of cable status'
Assert-Equal $result.linkStatus 'Disconnected' 'Disconnected link is preserved in receipt'

Reset-TestAdapter; $script:denyAdmin = $true
$denied = $false
try { Set-AdapterRandomMAC -AdapterId $script:adapter.InterfaceGuid -Mode 'static-random' | Out-Null } catch { $denied = $true }
Assert-Equal $denied $true 'Backend independently requires administrator'
Assert-Equal $script:registry.Count 0 'Denied operation is nonmutating'

Reset-TestAdapter
$result = Set-AdapterRandomMAC -AdapterId 'not-a-guid' -Mode 'static-random'
Assert-Equal $result.status 'blocked' 'Malformed identity rejected'
Assert-Equal $script:registry.Count 0 'Invalid identity is nonmutating'
Write-Output 'Adapter transaction tests passed (all Windows I/O mocked).'
