$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$workflow = Join-Path $repo '.github/workflows/invariants.yml'
$text = Get-Content -LiteralPath $workflow -Raw

$required = @(
    'pull_request:',
    'push:',
    'branches: [main]',
    'concurrency:',
    'cancel-in-progress: true',
    'rust-toolchain.toml',
    '.githooks/**',
    'tsconfig*.json',
    '.gitleaks.toml',
    'bun test',
    'cargo check --all-targets',
    'check-backend-leakage.ps1'
)
foreach ($needle in $required) {
    if (-not $text.Contains($needle)) {
        throw "invariants.yml is missing required CI contract text: $needle"
    }
}
if ($text.Contains('pull_request_target:')) {
    throw 'invariants.yml must not run repository code through pull_request_target.'
}
foreach ($job in @('cargo-check', 'cargo-test', 'cargo-clippy')) {
    $match = [regex]::Match($text, "(?ms)^  $([regex]::Escape($job)):\s*(.*?)(?=^  [a-z0-9-]+:|\z)")
    if (-not $match.Success -or -not $match.Groups[1].Value.Contains('bun run encrypt-backend')) {
        throw "invariants.yml Rust job '$job' must generate ignored backend modules before compiling."
    }
}
$stringsJob = [regex]::Match($text, '(?ms)^  strings-grep-free:\s*(.*?)(?=^  [a-z0-9-]+:|\z)')
if (-not $stringsJob.Success -or -not $stringsJob.Groups[1].Value.Contains('products/wincommander')) {
    throw "invariants.yml Free binary job must checkout the WinCommander product assets required by the production frontend build."
}
$actionRefs = [regex]::Matches($text, '(?m)^\s*(?:-\s*)?uses:\s*[^@\s]+@([^\s#]+)')
foreach ($match in $actionRefs) {
    if ($match.Groups[1].Value -notmatch '^[0-9a-f]{40}$') {
        throw "invariants.yml action reference is not pinned to a full commit SHA: $($match.Value.Trim())"
    }
}
Write-Host 'Free CI trigger and safety contract passed.'
