[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('Backup','Restore')][string]$Action,
    [Parameter(Mandatory)][string]$ServicePath,
    [Parameter(Mandatory)][string]$BackupPath,
    [string]$ExpectedHash
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
function Copy-VerifiedServiceImage([string]$Source, [string]$Destination, [string]$Expected = '') {
    $item = Get-Item -LiteralPath $Source -Force
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Service image is not a regular file.' }
    $hash = (Get-FileHash -LiteralPath $Source -Algorithm SHA256).Hash
    if ($Expected -and $hash -cne $Expected) { throw 'Service backup hash changed.' }
    [IO.File]::Copy($Source, $Destination, $true)
    if ((Get-FileHash -LiteralPath $Destination -Algorithm SHA256).Hash -cne $hash) { throw 'Service image copy did not verify.' }
    return $hash
}
if ($MyInvocation.InvocationName -eq '.') { return }
try {
    if ($Action -eq 'Backup') {
        Copy-VerifiedServiceImage $ServicePath $BackupPath
    } else {
        if ($ExpectedHash -cnotmatch '^[0-9A-F]{64}$') { throw 'Missing expected service backup hash.' }
        [void](Copy-VerifiedServiceImage $BackupPath $ServicePath $ExpectedHash)
    }
    exit 0
} catch {
    [Console]::Error.WriteLine('Could not safely preserve or restore the WinCommander service image.')
    exit 1
}
