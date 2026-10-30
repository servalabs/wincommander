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
    $script:denyFirst = $false
    $script:scanCalls = @{}
    function Erase-Dir {
        param([string]$Path)
        if ($script:deny -or ($script:denyFirst -and $Path -eq 'C:\Fixture\Roaming\npm-cache')) { throw 'Synthetic inventory access denied.' }
        $script:scanCalls[$Path] = 1
    }
    function Get-Item {
        [CmdletBinding()] param([string]$LiteralPath, [switch]$Force)
        if (-not $script:scanCalls.ContainsKey($LiteralPath)) { throw [Management.Automation.ItemNotFoundException]::new('Synthetic absent path') }
        $script:scanCalls[$LiteralPath]++
        if ($script:retained) { return [pscustomobject]@{ FullName = $LiteralPath } }
        throw [Management.Automation.ItemNotFoundException]::new('Synthetic absent path')
    }
    function Erase-OneFile { param([string]$Path) return $true }
    function Test-Path { param([string]$LiteralPath, $ErrorAction) return $false }
    foreach ($category in @('developerCaches', 'passwordManagerCaches')) {
        $script:scanCalls = @{}; $script:retained = $false
        Invoke-Expression $script:AutoEraseScripts[$category]
        if (@($script:scanCalls.Values | Where-Object { $_ -ne 2 }).Count) { throw "$category did not recheck every cache root." }
        $script:scanCalls = @{}; $script:retained = $true; $script:AutoEraseFailed = 0
        Invoke-Expression $script:AutoEraseScripts[$category]
        if ($script:AutoEraseFailed -eq 0) { throw "$category reported success despite remaining files." }
        $script:scanCalls = @{}; $script:deny = $true; $script:AutoEraseFailed = 0
        Invoke-Expression $script:AutoEraseScripts[$category]
        if ($script:AutoEraseFailed -eq 0) { throw "$category reported success despite inaccessible files." }
        $script:deny = $false
    }
    $script:scanCalls = @{}; $script:retained = $false; $script:denyFirst = $true; $script:AutoEraseFailed = 0
    Invoke-Expression $script:AutoEraseScripts['developerCaches']
    if ($script:AutoEraseFailed -eq 0 -or -not $script:scanCalls.ContainsKey('C:\Fixture\Local\npm-cache')) { throw 'An inaccessible first cache stopped cleanup of later roots.' }
    Write-Output 'Passed: protected cache inventory, post-clear readback and access failures.'
} finally {
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') }
}
