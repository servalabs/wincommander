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
'Hybrid Free publication gate passed.'
