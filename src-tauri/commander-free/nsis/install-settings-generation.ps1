[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$ExecutablePath,
    [ValidateSet("stage", "install", "new-identity")][string]$Action = "stage"
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
try {
    $resolved = [IO.Path]::GetFullPath($ExecutablePath)
    if (-not [IO.File]::Exists($resolved)) { throw 'Installed WinCommander executable is unavailable.' }
    # Child-only opt-in. The binary fails closed if it was built without the
    # experimental feature, if maintenance checks fail, or if data is ambiguous.
    $env:WINCOMMANDER_EXPERIMENTAL_SETTINGS_GENERATION = '1'
    if ($Action -eq 'new-identity') {
        & $resolved --settings-generation-lab new-machine-identity --offline-confirmed --confirm-new-identity
    } else {
        & $resolved --settings-generation-lab $Action --offline-confirmed
    }
    if ($LASTEXITCODE -eq 2) { exit 2 }
    if ($LASTEXITCODE -ne 0) { throw "Settings maintenance returned exit code $LASTEXITCODE." }
}
catch {
    Write-Error "Settings maintenance did not complete; existing files were preserved. $($_.Exception.Message)"
    exit 1
}
