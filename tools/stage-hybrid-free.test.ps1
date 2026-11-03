# SPDX-License-Identifier: AGPL-3.0-or-later
[CmdletBinding()]
param([Parameter(Mandatory = $true)][string]$ToolPath)
$ErrorActionPreference = 'Stop'
$root = Join-Path ([IO.Path]::GetTempPath()) ('wincmd-free-hybrid-stage-' + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $root
try {
    try {
        & (Join-Path $PSScriptRoot 'release-free.ps1') -Version '99.0.0' -HybridPrepareTool $ToolPath
        throw 'Publication accepted the hybrid rehearsal option.'
    } catch {
        if ($_.Exception.Message -notlike '*available only with -StageOnly*') { throw }
    }
    try {
        & (Join-Path $PSScriptRoot 'release-free.ps1') -Version '99.0.0-preview' -StageOnly -HybridPrepareTool $ToolPath
        throw 'Prerelease version was accepted.'
    } catch {
        if ($_.Exception.Message -notlike '*requires a numeric X.Y.Z version*') { throw }
    }
    $binary = Join-Path $root 'setup.exe'
    $manifest = Join-Path $root 'latest.json'
    $fixture = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../src-tauri/wincmd-pq-experiment/tests/fixtures/free-tauri-disposable.json') -Raw | ConvertFrom-Json
    $tauri = Join-Path $root 'tauri.json'
    [IO.File]::WriteAllText($tauri, (@{plugins=@{updater=@{pubkey=$fixture.publicKey}}} | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllText($binary, $fixture.installer, [Text.UTF8Encoding]::new($false))
    $hash = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant()
    $value = @{version='99.0.0';platforms=@{'windows-x86_64'=@{url='https://winupdates.servalabs.com/free/v99.0.0/WinCommander_99.0.0_x64-setup.exe';signature=$fixture.signature}}}
    [IO.File]::WriteAllText($manifest, ($value | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
    foreach ($channel in @('stable', 'beta')) {
        $output = Join-Path $root "$channel.json"
        & (Join-Path $PSScriptRoot 'stage-hybrid-free.ps1') -ToolPath $ToolPath -ManifestPath $manifest -InstallerPath $binary -TauriConfigPath $tauri -Version '99.0.0' -Channel $channel -OutputPath $output
        $payload = Get-Content -LiteralPath $output -Raw | ConvertFrom-Json
        if ($payload.format -ne 'wincommander-free-release-v1' -or $payload.channel -ne $channel -or $payload.sha256 -ne $hash) { throw 'Free payload does not match the staged release.' }
        try { & (Join-Path $PSScriptRoot 'stage-hybrid-free.ps1') -ToolPath $ToolPath -ManifestPath $manifest -InstallerPath $binary -TauriConfigPath $tauri -Version '99.0.0' -Channel $channel -OutputPath $output; throw 'Existing output was overwritten.' } catch { if ($_.Exception.Message -eq 'Existing output was overwritten.') { throw } }
    }
    $bad = Join-Path $root 'bad.json'
    [IO.File]::WriteAllText($binary, 'MX', [Text.UTF8Encoding]::new($false))
    try { & (Join-Path $PSScriptRoot 'stage-hybrid-free.ps1') -ToolPath $ToolPath -ManifestPath $manifest -InstallerPath $binary -TauriConfigPath $tauri -Version '99.0.0' -Channel stable -OutputPath $bad; throw 'Changed installer was accepted.' } catch { if ($_.Exception.Message -eq 'Changed installer was accepted.') { throw } }
    if (Test-Path -LiteralPath $bad) { throw 'Changed installer produced an output.' }
    [IO.File]::WriteAllText($binary, $fixture.installer, [Text.UTF8Encoding]::new($false))
    try { & (Join-Path $PSScriptRoot 'stage-hybrid-free.ps1') -ToolPath $ToolPath -ManifestPath $manifest -InstallerPath $binary -TauriConfigPath $tauri -Version '99.0.1' -Channel stable -OutputPath $bad; throw 'Wrong version was accepted.' } catch { if ($_.Exception.Message -eq 'Wrong version was accepted.') { throw } }
    if (Test-Path -LiteralPath $bad) { throw 'Invalid input produced an output.' }
    $value.platforms.'windows-x86_64'.url = 'https://example.invalid/setup.exe'
    [IO.File]::WriteAllText($manifest, ($value | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
    try { & (Join-Path $PSScriptRoot 'stage-hybrid-free.ps1') -ToolPath $ToolPath -ManifestPath $manifest -InstallerPath $binary -TauriConfigPath $tauri -Version '99.0.0' -Channel stable -OutputPath $bad; throw 'Wrong host was accepted.' } catch { if ($_.Exception.Message -eq 'Wrong host was accepted.') { throw } }
    if (Test-Path -LiteralPath $bad) { throw 'Invalid host produced an output.' }
    Write-Host 'Hybrid Free local staging integration passed.'
} finally {
    foreach ($name in @('setup.exe','tauri.json','latest.json','stable.json','beta.json','bad.json')) {
        $file = Join-Path $root $name
        if (Test-Path -LiteralPath $file -PathType Leaf) { Remove-Item -LiteralPath $file -Force }
    }
    Remove-Item -LiteralPath $root -Force
}
