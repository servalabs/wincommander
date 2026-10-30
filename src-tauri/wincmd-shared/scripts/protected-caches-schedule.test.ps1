$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')

# Provider calls are mocked so no local cache or credential file is read.
$saved = @{ USERPROFILE = $env:USERPROFILE; LOCALAPPDATA = $env:LOCALAPPDATA; APPDATA = $env:APPDATA }
try {
    $env:USERPROFILE = 'C:\Fixture\User'
    $env:LOCALAPPDATA = 'C:\Fixture\Local'
    $env:APPDATA = 'C:\Fixture\Roaming'
    $script:retained = $false
    $script:deny = $false
    $script:scanCalls = @{}
    function Get-AutoEraseFiles {
        param([string]$Path, [switch]$Recurse)
        if (-not $Recurse) { throw 'Expected recursive cache inventory.' }
        if ($script:deny) { throw 'Synthetic inventory access denied.' }
        if (-not $script:scanCalls.ContainsKey($Path)) { $script:scanCalls[$Path] = 0 }
        $script:scanCalls[$Path]++
        if ($script:scanCalls[$Path] -eq 1 -or $script:retained) { [pscustomobject]@{ FullName = "$Path\item.bin" } }
    }
    function Erase-FileSet { param([object[]]$Files) }
    function Erase-OneFile { param([string]$Path) return $true }
    function Test-Path { param([string]$LiteralPath, $ErrorAction) return $false }
    foreach ($category in @('developerCaches', 'passwordManagerCaches')) {
        $script:scanCalls = @{}; $script:retained = $false
        Invoke-Expression $script:AutoEraseScripts[$category]
        if (@($script:scanCalls.Values | Where-Object { $_ -ne 2 }).Count) { throw "$category did not recheck every cache root." }
        $script:scanCalls = @{}; $script:retained = $true; $failed = $false
        try { Invoke-Expression $script:AutoEraseScripts[$category] } catch { $failed = $true }
        if (-not $failed) { throw "$category reported success despite remaining files." }
        $script:scanCalls = @{}; $script:deny = $true; $failed = $false
        try { Invoke-Expression $script:AutoEraseScripts[$category] } catch { $failed = $true }
        if (-not $failed) { throw "$category reported success despite inaccessible files." }
        $script:deny = $false
    }
    Write-Output 'Passed: protected cache inventory, post-clear readback and access failures.'
} finally {
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') }
}
