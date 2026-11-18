$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '../../src-tauri/commander-free/scripts/modules/privacy/telemetry.ps1')

$script:accessFixture = 'unset'
function Get-ItemProperty {
    [CmdletBinding()] param([string]$Path, [string]$Name)
    if ($script:accessFixture -eq 'denied') { throw [System.UnauthorizedAccessException]::new('fixture denied') }
    if ($script:accessFixture -eq 'blocked' -and $Name -eq 'AllowCamera') { return [pscustomobject]@{ AllowCamera = 0 } }
    throw [System.Management.Automation.ItemNotFoundException]::new('fixture missing')
}
function Get-ChildItem {
    [CmdletBinding()] param([string]$Path, [switch]$Recurse)
    if ($script:accessFixture -eq 'children_denied') { throw [System.UnauthorizedAccessException]::new('fixture denied') }
    throw [System.Management.Automation.ItemNotFoundException]::new('fixture missing')
}

$unset = Get-AppCapabilityAccessStatus -Capability webcam
if ($unset.error -or $unset.disabled -ne $false) { throw 'Unset values should retain the existing Allow default' }
$script:accessFixture = 'blocked'
$blocked = Get-AppCapabilityAccessStatus -Capability webcam
if ($blocked.error -or $blocked.disabled -ne $true) { throw 'A confirmed camera policy denial must remain blocked' }
foreach ($fixture in @('denied', 'children_denied')) {
    $script:accessFixture = $fixture
    $result = Get-AppCapabilityAccessStatus -Capability webcam
    if (-not $result.error -or $null -ne $result.disabled) { throw "A $fixture read must remain unknown" }
}
Write-Output 'PASS: unset, blocked and denied camera policy reads'

# Model registry values only. No test read or write reaches the real registry.
$script:fakeRegistry = @{}
$script:elevated = $false
function Test-IsAdmin { $script:elevated }
function Assert-IsAdmin { if (-not $script:elevated) { throw 'Fixture is not elevated' } }
function Test-Path { [CmdletBinding()] param([string]$LiteralPath) $script:fakeRegistry.ContainsKey($LiteralPath) }
function New-Item { [CmdletBinding()] param([string]$Path, [switch]$Force) $script:fakeRegistry[$Path] = @{} }
function Set-ItemProperty {
    [CmdletBinding()] param([string]$LiteralPath, [string]$Name, $Value, [string]$Type, [switch]$Force)
    $script:fakeRegistry[$LiteralPath][$Name] = $Value
}
function Get-ItemProperty {
    [CmdletBinding()] param([string]$Path, [string]$LiteralPath, [string]$Name)
    $key = if ($LiteralPath) { $LiteralPath } else { $Path }
    if (-not $script:fakeRegistry.ContainsKey($key)) {
        if ($PSBoundParameters.ErrorAction -eq 'SilentlyContinue') { return }
        throw [System.Management.Automation.ItemNotFoundException]::new('Fixture key missing')
    }
    [pscustomobject]$script:fakeRegistry[$key]
}
function Get-ChildItem { [CmdletBinding()] param([string]$Path, [string]$LiteralPath, [switch]$Recurse) }
function Set-RegistryValueSafe {
    param([string]$Path, [string]$Name, $Value, [string]$Type)
    if (-not $script:fakeRegistry.ContainsKey($Path)) { $script:fakeRegistry[$Path] = @{} }
    $script:fakeRegistry[$Path][$Name] = $Value
}
function Remove-ItemProperty {
    [CmdletBinding()] param([string]$Path, [string]$Name)
    if ($script:fakeRegistry.ContainsKey($Path)) { $script:fakeRegistry[$Path].Remove($Name) }
}
function Add-Type { [CmdletBinding()] param([string]$TypeDefinition) throw 'Policy refresh is suppressed in the fixture' }

foreach ($script:elevated in @($false, $true)) {
    foreach ($capability in @('webcam', 'microphone')) {
        $script:fakeRegistry = @{}
        foreach ($value in @('Deny', 'Allow')) {
            $result = Set-AppCapabilityAccess -Capability $capability -Access $value
            if (-not $result.verified -or $result.value -ne $value) {
                throw "$capability $value did not round-trip; administrator=$script:elevated, error=$($result.message)"
            }
        }
        $machineWrites = @($script:fakeRegistry.Keys | Where-Object { $_ -like 'HKLM:*' }).Count
        if (($machineWrites -gt 0) -ne $script:elevated) { throw 'Writes escaped the fixture account scope' }
    }
}
Write-Output 'PASS: camera and microphone consent round-trip without hardware for standard and elevated accounts'
