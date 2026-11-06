$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $path), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup source did not parse.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-WCAmcacheSnapshotInventory' }, $true)
if (-not $definition) { throw 'Snapshot integration is missing.' }
. ([scriptblock]::Create($definition.Extent.Text))
$entryDefinition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-AmcacheEntries' }, $true)
if (-not $entryDefinition) { throw 'Amcache scan is missing.' }
. ([scriptblock]::Create($entryDefinition.Extent.Text))

$original = $env:WINCMD_EXE_PATH
$originalAvailable = $env:WINCMD_AMCACHE_SNAPSHOT_AVAILABLE
$env:WINCMD_EXE_PATH = 'Invoke-FakeAmcacheCollector'
$env:WINCMD_AMCACHE_SNAPSHOT_AVAILABLE = '1'
$script:result = @{
    status = 'complete'; total = 1; snapshotAt = '2026-01-01T00:00:00Z'; entries = @(
        @{ category = 'InventoryApplication'; count = 1; sample = @(@{ id='synthetic'; name='app'; path='C:\app.exe'; registryValues=@{ Publisher='synthetic' } }) },
        @{ category = 'InventoryApplicationFile'; count = 0; sample = @() },
        @{ category = 'InventoryApplicationShortcut'; count = 0; sample = @() },
        @{ category = 'InventoryDeviceContainer'; count = 0; sample = @() },
        @{ category = 'InventoryDriverBinary'; count = 0; sample = @() }
    )
}
function Invoke-WCAmcacheNativeCollector {
    $script:result | ConvertTo-Json -Depth 10 -Compress
}
function Get-Item {
    [CmdletBinding()] param([string]$LiteralPath, [string]$Path)
    if ($LiteralPath -like '*Amcache.hve') { return [pscustomobject]@{ Length = 1024 } }
    throw [System.Management.Automation.ItemNotFoundException]::new('Not mounted')
}
try {
    $local = Get-WCAmcacheSnapshotInventory
    if (-not $local -or $local.total -ne 1 -or $local.entries[0].sample[0].registryValues.Publisher -ne 'synthetic') { throw 'A valid local snapshot was not retained.' }
    $fleet = Get-WCAmcacheSnapshotInventory -FleetProjection
    if (-not $fleet -or $fleet.entries[0].sample[0].ContainsKey('registryValues')) { throw 'Fleet received local-only registry values.' }
    $scan = Get-AmcacheEntries
    if ($scan.status -ne 'complete' -or $scan.source -ne 'snapshot' -or $scan.total -ne 1 -or -not $scan.hveFileExists) { throw 'The cleanup scan did not use the verified snapshot.' }
    $script:result.total = 2
    if ($null -ne (Get-WCAmcacheSnapshotInventory)) { throw 'An inconsistent count was accepted.' }
    if (-not (Get-AmcacheEntries).partial) { throw 'An invalid snapshot did not fall back to unavailable.' }
    $script:result.total = 1; $script:result.status = 'partial'
    if ($null -ne (Get-WCAmcacheSnapshotInventory)) { throw 'A partial snapshot was accepted as complete.' }
    $script:result.status = 'complete'; $script:result.entries = @($script:result.entries | Select-Object -First 4)
    if ($null -ne (Get-WCAmcacheSnapshotInventory)) { throw 'A missing category was accepted.' }
} finally { $env:WINCMD_EXE_PATH = $original; $env:WINCMD_AMCACHE_SNAPSHOT_AVAILABLE = $originalAvailable }
Write-Output 'Passed: complete snapshot, Fleet minimization, and invalid result rejection.'
