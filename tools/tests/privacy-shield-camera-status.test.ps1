$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '../../src-tauri/commander-free/scripts/modules/privacy/privacy_shield.ps1')

$script:cameraFixture = 'present'
function Get-CimInstance {
    [CmdletBinding()] param([string]$ClassName, [string]$Filter)
    if ($ClassName -eq 'Win32_OperatingSystem') {
        return [pscustomobject]@{ ProductType = if ($script:cameraFixture -like 'server_*') { 3 } else { 1 } }
    }
    if ($ClassName -eq 'Win32_Process') { return }
    if ($script:cameraFixture -eq 'wmi_failed') { throw 'fixture WMI failure' }
    if ($Filter -eq "PNPClass='Camera'" -and $script:cameraFixture -in @('present', 'blocked', 'remote_host_camera', 'remote_virtual')) {
        return [pscustomobject]@{ Name='Synthetic camera'; Status='OK'; Present=$true }
    }
}
function Get-PrivacyShieldVideoSourceSummary {
    if ($script:cameraFixture -in @('virtual', 'remote_virtual')) { return @{ known=$true; count=1; access='Allowed' } }
    if ($script:cameraFixture -in @('restricted', 'server_restricted')) { return @{ known=$true; count=0; access='DeniedByUser' } }
    if ($script:cameraFixture -eq 'server_api_unavailable') { return @{ known=$false; count=0; access='Unknown' } }
    return @{ known=$true; count=0; access='Allowed' }
}
function Get-PrivacyShieldRemoteSession { return ($script:cameraFixture -like 'remote_*' -or $script:cameraFixture -like 'server_*') }
function Get-PrivacyShieldPidFromMarker { return $null }
function Get-PnpDevice {
    [CmdletBinding()] param([switch]$PresentOnly)
    if ($script:cameraFixture -eq 'ambiguous') { return [pscustomobject]@{ Class='Media' } }
}
function Get-ItemProperty {
    [CmdletBinding()] param([string]$Path, [string]$Name)
    if ($script:cameraFixture -eq 'blocked' -and $Name -eq 'AllowCamera') { return [pscustomobject]@{ AllowCamera=0 } }
    return $null
}
function Assert-Status($fixture, $expected, $available, $presence) {
    $script:cameraFixture = $fixture
    $result = Get-PrivacyShieldCameraAvailability
    if ($result.cameraStatus -ne $expected -or $result.available -ne $available) {
        throw "$fixture returned status $($result.cameraStatus), available $($result.available)"
    }
    if ($result.cameraPresence -ne $presence) { throw "$fixture returned presence $($result.cameraPresence)" }
    if (@($result.devices) | Where-Object { $_ -ne 'Camera source' }) { throw 'Device details leaked' }
}

Assert-Status 'present' 'available' $true 'present'
Assert-Status 'virtual' 'available' $true 'present'
Assert-Status 'remote_host_camera' 'unknown' $null 'unknown'
Assert-Status 'remote_virtual' 'available' $true 'present'
Assert-Status 'absent' 'hardware_unavailable' $false 'absent'
Assert-Status 'wmi_failed' 'unknown' $null 'unknown'
Assert-Status 'restricted' 'unknown' $null 'host_absent'
Assert-Status 'server_restricted' 'unknown' $null 'host_absent'
Assert-Status 'server_api_unavailable' 'unknown' $null 'host_absent'
Assert-Status 'ambiguous' 'hardware_unavailable' $false 'absent'
Assert-Status 'blocked' 'windows_camera_policy_denied' $false 'present'
$script:cameraFixture = 'server_restricted'
$serverStatus = Get-PrivacyShieldStatus
if (-not $serverStatus.isWindowsServer -or $serverStatus.cameraAvailable -ne $false -or $serverStatus.cameraStatus -ne 'unknown' -or $serverStatus.cameraPresence -ne 'host_absent' -or $serverStatus.cameraMessage -notmatch 'redirected') {
    throw 'Server must report host absence without claiming the redirected camera was checked or relaxing the Fleet guard'
}
$script:cameraFixture = 'wmi_failed'
$status = Get-PrivacyShieldStatus
if ($status.cameraAvailable -ne $false -or $status.cameraStatus -ne 'unknown') {
    throw 'Unknown camera status must retain the Fleet pre-stop guard without reporting missing hardware'
}
foreach ($line in @('Camera initialization timed out', 'Camera could not be opened', 'No camera detected - VideoCapture timed out')) {
    if ((Get-PrivacyShieldStartFailure $line).code -ne 'camera_start_unverified') { throw "Incorrect open failure classification: $line" }
}
if ((Get-PrivacyShieldStartFailure 'Camera currently in use').code -ne 'camera_busy') { throw 'Busy camera classification changed' }
if ((Get-PrivacyShieldStartFailure 'Camera feed unavailable: black frames').code -ne 'camera_feed_unavailable') { throw 'Black-feed classification changed' }
$path = Join-Path ([System.IO.Path]::GetTempPath()) ("wc-shield-fixture-" + [guid]::NewGuid().ToString('N') + '.log')
try {
    $old = 'a' * 32
    $current = 'b' * 32
    Set-Content -LiteralPath $path -Value @("[time] [$old] Camera currently in use", "[time] [$current] Model preparation failed")
    $tail = Get-PrivacyShieldLaunchLogTail -Path $path -RunId $current
    if ((Get-PrivacyShieldStartFailure $tail).code -ne 'model_download_failed') { throw 'A previous launch contaminated the current diagnosis' }
} finally { Remove-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue }

# An inconclusive inventory must not stop a local user from attempting a
# detector open, while explicit policy denial and unknown lifecycle stay closed.
function Get-PrivacyShieldStatus {
    if ($script:cameraFixture -eq 'status_error') { return @{ error=$true; shieldLifecycle='unknown' } }
    if ($script:cameraFixture -eq 'blocked') { return @{ running=$false; cameraAvailable=$false; cameraStatus='windows_camera_policy_denied'; shieldLifecycle='stopped' } }
    return @{ running=$false; cameraAvailable=$false; cameraStatus='unknown'; shieldLifecycle='stopped' }
}
function Resolve-PythonPath { return $null }
$script:cameraFixture = 'wmi_failed'
$unknownRuntime = Start-PrivacyShield
if ($unknownRuntime.diagnosticCode -ne 'runtime_unavailable') { throw 'Unknown camera must reach the runtime check' }
if ($unknownRuntime.cameraStatus -ne 'unknown') { throw 'Missing runtime must not mark an unknown camera available' }
$script:cameraFixture = 'blocked'
if ((Start-PrivacyShield).diagnosticCode -ne 'windows_camera_policy_denied') { throw 'Known camera policy denial must prevent startup' }
$script:cameraFixture = 'status_error'
if ((Start-PrivacyShield).diagnosticCode -ne 'status_unavailable') { throw 'Unknown shield lifecycle must prevent startup' }
Write-Output 'PASS: camera presence, virtual source, unknown coverage, policy denial, and open-failure classification'
