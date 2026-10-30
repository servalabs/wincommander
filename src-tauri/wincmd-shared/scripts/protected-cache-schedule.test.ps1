$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')
. ([scriptblock]::Create($script:EraseFunctions))

$original = @{ APPDATA=$env:APPDATA; LOCALAPPDATA=$env:LOCALAPPDATA; USERPROFILE=$env:USERPROFILE }
$fixture = Join-Path $env:TEMP ('wc-cache-test-' + [guid]::NewGuid().ToString('N'))
try {
    $env:APPDATA = Join-Path $fixture 'roaming'
    $env:LOCALAPPDATA = Join-Path $fixture 'local'
    $env:USERPROFILE = Join-Path $fixture 'user'
    foreach ($path in @($env:APPDATA, $env:LOCALAPPDATA, $env:USERPROFILE)) { New-Item -ItemType Directory -Path $path -Force | Out-Null }
    foreach ($case in @(
        @{ id='developerCaches'; root=(Join-Path $env:LOCALAPPDATA 'pip\Cache') },
        @{ id='passwordManagerCaches'; root=(Join-Path $env:APPDATA 'Bitwarden\Cache') }
    )) {
        $nested = Join-Path $case.root 'sensitive-name\empty-child'
        New-Item -ItemType Directory -Path $nested -Force | Out-Null
        Set-Content -LiteralPath (Join-Path $case.root 'synthetic.txt') -Value 'fixture'
        $script:AutoEraseFailed = 0; $script:AutoEraseRemoved = 0; $script:AutoEraseDeferred = 0
        & ([scriptblock]::Create($script:AutoEraseScripts[$case.id]))
        if ($script:AutoEraseFailed -ne 0 -or (Test-Path -LiteralPath $case.root)) { throw "$($case.id) failed=$script:AutoEraseFailed exists=$(Test-Path -LiteralPath $case.root) removed=$script:AutoEraseRemoved" }
    }

    # An enumeration failure must be recorded; nothing may fall back to a recursive delete.
    $root = Join-Path $env:LOCALAPPDATA 'pip\Cache'
    New-Item -ItemType Directory -Path $root -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $root 'must-remain.txt') -Value 'fixture'
    function Get-AutoEraseFiles { throw 'Synthetic reparse point' }
    $script:AutoEraseFailed = 0
    & ([scriptblock]::Create($script:AutoEraseScripts['developerCaches']))
    if ($script:AutoEraseFailed -eq 0 -or -not (Test-Path -LiteralPath (Join-Path $root 'must-remain.txt'))) { throw 'A rejected cache tree must remain intact and report failure.' }
} finally {
    $env:APPDATA = $original.APPDATA; $env:LOCALAPPDATA = $original.LOCALAPPDATA; $env:USERPROFILE = $original.USERPROFILE
    if (Test-Path -LiteralPath $fixture) { Remove-Item -LiteralPath $fixture -Recurse -Force }
}
Write-Output 'Passed: protected cache directory removal and rejected-tree preservation.'

