$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')

# These paths and files exist only in the mocked inventory.
$oldProfile = $env:USERPROFILE
$oldTemp = $env:TEMP
try {
    $env:USERPROFILE = 'C:\Fixture\User'
    $env:TEMP = 'C:\Fixture\Temp'
    $script:remaining = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    1..501 | ForEach-Object { [void]$script:remaining.Add("C:\Fixture\Temp\$_.tmp") }
    function Get-AutoEraseFiles {
        param([string]$Path, [string]$Filter, [switch]$Recurse, [int]$Depth)
        if (-not $Recurse -or $Depth -ne 2) { throw 'Unexpected scan scope.' }
        if ($Path -eq $env:TEMP -and $Filter -eq '*.tmp') {
            return @($script:remaining | ForEach-Object { [pscustomobject]@{ FullName = $_ } })
        }
        return @()
    }
    function Erase-FileSet {
        param([object[]]$Files)
        foreach ($file in $Files) { [void]$script:remaining.Remove($file.FullName) }
    }
    Invoke-Expression $script:AutoEraseScripts['officeTempFiles']
    if ($script:remaining.Count -ne 0) { throw 'More than 500 temporary files were not removed.' }

    [void]$script:remaining.Add('C:\Fixture\Temp\locked.tmp')
    function Erase-FileSet { param([object[]]$Files) }
    $failed = $false
    try { Invoke-Expression $script:AutoEraseScripts['officeTempFiles'] } catch { $failed = $true }
    if (-not $failed) { throw 'A remaining file was reported as cleared.' }
    Write-Output 'Passed: all Office temporary files, including beyond 500, and remaining-file readback.'
} finally {
    $env:USERPROFILE = $oldProfile
    $env:TEMP = $oldTemp
}
