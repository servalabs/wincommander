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
