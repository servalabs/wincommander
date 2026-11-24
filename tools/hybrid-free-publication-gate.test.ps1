# SPDX-License-Identifier: AGPL-3.0-or-later
$ErrorActionPreference = 'Stop'
$publisher = Join-Path $PSScriptRoot 'release-free.ps1'
function Expect-Refusal([hashtable]$Parameters, [string]$Expected) {
    try {
        & $publisher @Parameters
        throw 'Publisher unexpectedly continued.'
    } catch {
        if ($_.Exception.Message -notlike "*$Expected*") { throw }
    }
}
# These must fail at argument validation, before reading .env, building, or
# reaching the network-capable publication code.
Expect-Refusal @{ Version = '99.0.0' } 'Free publication is disabled'
Expect-Refusal @{ Version = '99.0.0'; HybridSignedRecord = 'fake.json' } 'requires -StageOnly -SkipBuild'
Expect-Refusal @{ Version = '99.0.0'; StageOnly = $true; HybridSignedRecord = 'fake.json' } 'requires -StageOnly -SkipBuild'
$priorProfile = $env:WINCMD_RELEASE_PROFILE
try {
    # An explicit hybrid or empty profile is the default: publication stays disabled.
    foreach ($value in @('hybrid', '')) {
        $env:WINCMD_RELEASE_PROFILE = $value
        Expect-Refusal @{ Version = '99.0.0' } 'Free publication is disabled'
    }
    # Anything but the exact values fails closed, including for -StageOnly.
    foreach ($value in @('Classical', 'pq', ' classical')) {
        $env:WINCMD_RELEASE_PROFILE = $value
        Expect-Refusal @{ Version = '99.0.0' } 'Unknown release profile'
        Expect-Refusal @{ Version = '99.0.0'; StageOnly = $true } 'Unknown release profile'
    }
    # Classical is deliberately not exercised here: it passes this gate and would
    # proceed to read the private release environment. See release-free.contract.test.ts.
} finally {
    if ($null -eq $priorProfile) { Remove-Item Env:WINCMD_RELEASE_PROFILE -ErrorAction SilentlyContinue } else { $env:WINCMD_RELEASE_PROFILE = $priorProfile }
}
'Hybrid Free publication gate passed.'
