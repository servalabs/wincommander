# First-run-safe entry point for the full Tauri development environment.

[CmdletBinding()]
param(
    [switch]$ServerOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
& (Join-Path $PSScriptRoot "ensure-dev-environment.ps1")

$cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
if ((Test-Path -LiteralPath $cargoBin) -and -not (($env:Path -split ";") -contains $cargoBin)) {
    $env:Path = "$cargoBin;$env:Path"
}

$bun = Get-Command bun -CommandType Application -ErrorAction SilentlyContinue |
    Select-Object -First 1 -ExpandProperty Path
if (-not $bun) {
    $bun = Join-Path (if ($env:BUN_INSTALL) { $env:BUN_INSTALL } else { Join-Path $env:USERPROFILE ".bun" }) "bin\bun.exe"
}
if (-not (Test-Path -LiteralPath $bun)) {
    throw "bun.exe was not found after the development environment bootstrap."
}

Push-Location $repoRoot
try {
    if ($ServerOnly) {
        & $bun run dev:server
    } else {
        & $bun x tauri dev --config src-tauri/commander-free/tauri.conf.json
    }
    $exitCode = $LASTEXITCODE
} finally {
    Pop-Location
}

exit $exitCode
