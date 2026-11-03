# SPDX-License-Identifier: AGPL-3.0-or-later
# Local rehearsal only. This never loads signing keys or publishes artifacts.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$ToolPath,
    [Parameter(Mandatory = $true)][string]$ManifestPath,
    [Parameter(Mandatory = $true)][string]$InstallerPath,
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][ValidateSet('stable', 'beta')][string]$Channel,
    [Parameter(Mandatory = $true)][string]$OutputPath
)
$ErrorActionPreference = 'Stop'
foreach ($path in @($ToolPath, $ManifestPath, $InstallerPath)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Hybrid Free rehearsal input is missing: $path" }
}
if (Test-Path -LiteralPath $OutputPath) { throw 'Hybrid Free rehearsal output already exists; choose a fresh staging location.' }
& $ToolPath stage $ManifestPath $InstallerPath $Version $Channel $OutputPath
if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $OutputPath -PathType Leaf)) {
    throw 'Hybrid Free rehearsal staging failed; no release was published.'
}
Write-Host '[staged] Experimental Free hybrid payload prepared locally; no signatures were produced.'
