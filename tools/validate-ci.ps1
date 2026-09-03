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
$actionRefs = [regex]::Matches($text, '(?m)^\s*(?:-\s*)?uses:\s*[^@\s]+@([^\s#]+)')
foreach ($match in $actionRefs) {
    if ($match.Groups[1].Value -notmatch '^[0-9a-f]{40}$') {
        throw "invariants.yml action reference is not pinned to a full commit SHA: $($match.Value.Trim())"
    }
}
Write-Host 'Free CI trigger and safety contract passed.'
