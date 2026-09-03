param([switch]$Uninstall)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
Push-Location $repo
try {
    $current = (git config --local --get core.hooksPath 2>$null)
    if ($Uninstall) {
        if ($current -eq '.githooks') {
            git config --local --unset core.hooksPath
            if ($LASTEXITCODE -ne 0) { throw 'Could not remove the repository hook configuration.' }
            Write-Host 'WinCommander pre-push hook disabled. The tracked hook remains available.'
        } elseif ($current) {
            throw "Refusing to remove another hook path: $current"
        } else {
            Write-Host 'No repository hook is configured.'
        }
        exit 0
    }
    if ($current -and $current -ne '.githooks') {
        throw "Refusing to replace the existing hook path: $current"
    }
    if (-not (Test-Path -LiteralPath (Join-Path $repo '.githooks/pre-push'))) {
        throw 'The tracked pre-push hook is missing.'
    }
    git config --local core.hooksPath .githooks
    if ($LASTEXITCODE -ne 0) { throw 'Could not configure the repository hook path.' }
    Write-Host 'WinCommander pre-push hook enabled. CI remains authoritative.'
} finally {
    Pop-Location
}
