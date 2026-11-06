[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$ExecutablePath,
    [ValidateSet("stage", "install", "new-identity")][string]$Action = "stage",
    [string]$SourceProof = ''
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
try {
    $resolved = [IO.Path]::GetFullPath($ExecutablePath)
    if (-not [IO.File]::Exists($resolved)) { throw 'Installed WinCommander executable is unavailable.' }
    # Child-only opt-in. The binary fails closed if it was built without the
    # experimental feature, if maintenance checks fail, or if data is ambiguous.
    $env:WINCOMMANDER_EXPERIMENTAL_SETTINGS_GENERATION = '1'
    if ($Action -eq 'stage') {
        $lines = @(& $resolved --settings-source-proof-lab stage --offline-confirmed)
        $nativeExit = $LASTEXITCODE
        if ($nativeExit -ne 0 -and $nativeExit -ne 2) { throw "Settings preflight returned exit code $nativeExit." }
        if ($lines.Count -ne 1 -or $lines[0] -cnotmatch '^(F0{64}|[LPA][0-9a-f]{64})$') {
            throw 'Settings preflight did not return one valid source proof.'
        }
        Write-Output $lines[0]
        exit $nativeExit
    } elseif ($Action -eq 'install') {
        if ($SourceProof -cnotmatch '^(F0{64}|[LPA][0-9a-f]{64})$') { throw 'Settings source proof is invalid or missing.' }
        & $resolved --settings-source-proof-lab install --offline-confirmed $SourceProof
    } elseif ($Action -eq 'new-identity') {
        & $resolved --settings-generation-lab new-machine-identity --offline-confirmed --confirm-new-identity
    }
    if ($LASTEXITCODE -eq 2) { exit 2 }
    if ($LASTEXITCODE -ne 0) { throw "Settings maintenance returned exit code $LASTEXITCODE." }
}
catch {
    Write-Error "Settings maintenance did not complete; existing files were preserved. $($_.Exception.Message)"
    exit 1
}
