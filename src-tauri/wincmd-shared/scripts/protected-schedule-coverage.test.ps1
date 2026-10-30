$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')

# Exercise the actual Docker helper with all system/provider calls mocked.
. ([scriptblock]::Create($script:ScheduledDockerFunctions))
function New-Item { [CmdletBinding()] param($ItemType, $Path) return $null }
function Test-Path { [CmdletBinding()] param($LiteralPath, $PathType) return $false }
function Erase-OneFile { param($Path) }
function Get-WCDockerDesktopIdentity { param($ConfigPath) return 'fixture' }
function Get-WCDockerResources { param($ConfigPath, $Identity) return @{ containers=@(); images=@(); volumes=@() } }
function Get-WCDockerLocalVolumes { param($ConfigPath, $Identity, $VolumeNames) return @{ eligible=@(); excluded=0 } }
function Invoke-WCCheckedDocker { param($ConfigPath, $Identity, $Arguments, $Mutation) return @() }
function Get-WCDockerBuildCacheCount { param($ConfigPath, $Identity) return 0 }
function Get-WCDockerArtifactRemainders { return 0 }
$docker = Invoke-WCDockerDesktopCleanup
if ($docker.failed -ne 0 -or -not $docker.coverageIncomplete) { throw 'Docker custom disk uncertainty was incorrectly reported as a deletion failure.' }

# Mock provider results: no Docker, virtual-machine or filesystem operations run.
function Invoke-WCDockerDesktopCleanup { return @{ removed=2; failed=0; coverageIncomplete=$true } }
function Invoke-WCVirtualMachineArtifactCleanup { return @{ removed=2; failed=0 } }
foreach ($category in @('dockerDesktopData', 'virtualMachineArtifacts')) {
    $script:AutoEraseRemoved=0; $script:AutoEraseFailed=0; $script:AutoEraseCoverageIncomplete=$false
    Invoke-Expression $script:AutoEraseScripts[$category]
    if ($script:AutoEraseRemoved -ne 2 -or $script:AutoEraseFailed -ne 0 -or -not $script:AutoEraseCoverageIncomplete) { throw "$category did not separately report coverage and successful removals." }
}
function Invoke-WCDockerDesktopCleanup { throw 'Synthetic provider failure' }
function Invoke-WCVirtualMachineArtifactCleanup { throw 'Synthetic provider failure' }
foreach ($category in @('dockerDesktopData', 'virtualMachineArtifacts')) {
    $script:AutoEraseRemoved=0; $script:AutoEraseFailed=0; $script:AutoEraseCoverageIncomplete=$false
    Invoke-Expression $script:AutoEraseScripts[$category]
    if ($script:AutoEraseFailed -ne 1 -or -not $script:AutoEraseCoverageIncomplete) { throw "$category did not preserve a provider failure." }
}
Write-Output 'Passed: protected scheduled tasks distinguish limited coverage from provider failures.'
