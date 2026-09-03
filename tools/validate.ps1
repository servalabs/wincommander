param(
    [ValidateSet('Local', 'PrePush')]
    [string]$Mode = 'Local',
    [string]$Base = ''
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

if ($env:WINCOMMANDER_PREPUSH_FORCE_FAILURE -eq '1') {
    [Console]::Error.WriteLine('Intentional pre-push failure requested.')
    exit 90
}

function Invoke-Checked([string]$Label, [scriptblock]$Command) {
    $watch = [Diagnostics.Stopwatch]::StartNew()
    & $Command
    $code = $LASTEXITCODE
    $watch.Stop()
    if ($code -ne 0) {
        throw "$Label failed with exit code $code after $($watch.ElapsedMilliseconds) ms"
    }
    Write-Host "$Label passed in $($watch.ElapsedMilliseconds) ms"
}

Push-Location $repo
try {
    $changed = @()
    if ($Base -and $Base -notmatch '^0+$') {
        $changed = @(git diff --name-only "$Base..HEAD")
        if ($LASTEXITCODE -ne 0) { throw "Could not resolve pre-push base $Base" }
    }
    $rustChanged = $changed.Count -eq 0 -or $changed.Where({ $_ -like 'src-tauri/*' }).Count -gt 0

    Invoke-Checked 'CI contract' { powershell -NoProfile -ExecutionPolicy Bypass -File tools/validate-ci.ps1 }
    Invoke-Checked 'Bun tests' { bun test }
    Invoke-Checked 'TypeScript' { bun x tsc --noEmit }
    Invoke-Checked 'Tier invariants' { bun run lint:tiers }
    Invoke-Checked 'Generated types' { bun run gen:types:check }
    if ($rustChanged) {
        Invoke-Checked 'Rust check' { cargo check --manifest-path src-tauri/Cargo.toml -p commander-free -p commander-svc }
    } else {
        Write-Host 'Rust check skipped because this push has no src-tauri changes.'
    }
} finally {
    Pop-Location
}
