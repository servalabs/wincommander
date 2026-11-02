[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$FreeCommit,
    [Parameter(Mandatory = $true)][string]$FreeTree,
    [Parameter(Mandatory = $true)][string]$ManifestPath,
    [Parameter(Mandatory = $true)][string]$BinaryPath,
    [Parameter(Mandatory = $true)][string]$ReceiptPath,
    [Parameter(Mandatory = $true)][string]$UpdateDomain
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($Version -notmatch '^\d+\.\d+\.\d+$' -or $FreeCommit -notmatch '^[0-9a-fA-F]{40}$' -or $FreeTree -notmatch '^[0-9a-fA-F]{40}$') {
    throw 'Invalid paired release identity.'
}
$domain = $UpdateDomain.Trim().TrimEnd('/')
if ($domain -notmatch '^https://[^/]+$') {
    throw 'Paired update domain must be a HTTPS origin.'
}
$manifest = Get-Content -LiteralPath $ManifestPath -Raw | ConvertFrom-Json
$receipt = Get-Content -LiteralPath $ReceiptPath -Raw | ConvertFrom-Json
$binary = Get-Item -LiteralPath $BinaryPath
$binaryHash = (Get-FileHash -LiteralPath $BinaryPath -Algorithm SHA256).Hash.ToLowerInvariant()
$manifestHash = (Get-FileHash -LiteralPath $ManifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
$expectedUrl = "$domain/pro/v$Version/wincommander-pro.exe"
if ($manifest.version -ne $Version -or $manifest.url -cne $expectedUrl -or
    $manifest.sha256 -cne $binaryHash -or [int64]$manifest.size -ne $binary.Length -or
    $receipt.version -ne $Version -or $receipt.free_commit -cne $FreeCommit -or
    $receipt.free_tree -cne $FreeTree -or
    $receipt.pro_commit -notmatch '^[0-9a-fA-F]{40}$' -or
    $receipt.pro_tree -notmatch '^[0-9a-fA-F]{40}$' -or
    $receipt.pro_sha256 -cne $binaryHash -or
    $receipt.manifest_sha256 -cne $manifestHash -or
    [int64]$receipt.size -ne $binary.Length) {
    throw 'Pro release receipt, manifest, and binary do not match the selected Free beta commit.'
}
