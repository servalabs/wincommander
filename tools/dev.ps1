# First-run-safe entry point for the full Tauri development environment.

[CmdletBinding()]
param(
    [switch]$ServerOnly,
    # Relaunch this development entry point through Windows' explicit RunAs
    # path.  Debug builds intentionally use asInvoker, so an Administrator
    # account alone is not enough for privileged Vault-policy testing.
    [switch]$Elevated
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Test-ElevatedToken {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

if ($Elevated -and -not (Test-ElevatedToken)) {
    # Do not attempt to infer elevation from account membership or the UAC
    # slider.  Ask Windows for an explicit elevated parent process, then let
    # the debug executable inherit that token.
    $scriptPath = '"' + $PSCommandPath.Replace('"', '""') + '"'
    # Keep the elevated terminal open after an error so a developer can see
    # the real build/service failure instead of getting a vanished window.
    $arguments = "-NoExit -NoProfile -ExecutionPolicy Bypass -File $scriptPath -Elevated"
    if ($ServerOnly) { $arguments += " -ServerOnly" }
    Start-Process -FilePath powershell.exe -Verb RunAs -ArgumentList $arguments
    exit 0
}

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
        # Release an old debug executable before Tauri starts its build.  The
        # beforeDevCommand intentionally preserves the new app startup, so
        # this outer entry point owns stale-window cleanup.
        & (Join-Path $PSScriptRoot "kill-dev.ps1")
        & $bun x tauri dev --config src-tauri/commander-free/tauri.conf.json
    }
    $exitCode = $LASTEXITCODE
} finally {
    Pop-Location
}

exit $exitCode
