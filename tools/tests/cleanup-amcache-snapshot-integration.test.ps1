$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $path), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup source did not parse.' }
$failureDefinition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-WCAmcacheSnapshotFailureMessage' }, $true)
if (-not $failureDefinition) { throw 'Snapshot diagnostic mapping is missing.' }
. ([scriptblock]::Create($failureDefinition.Extent.Text))
if ((Get-WCAmcacheSnapshotFailureMessage 'At least 1 GiB of free space is required') -notmatch '1 GiB') { throw 'Low disk space was not explained.' }
if ((Get-WCAmcacheSnapshotFailureMessage 'Unable to create snapshot (Windows error 0x80070005).') -notmatch 'administrator') { throw 'Access denied was not explained.' }
if ((Get-WCAmcacheSnapshotFailureMessage 'The temporary snapshot could not be confirmed removed (Windows error 0x80070005).') -notmatch 'could not confirm removal') { throw 'Snapshot cleanup failure was masked as access denied.' }
if ((Get-WCAmcacheSnapshotFailureMessage 'The temporary snapshot could not be confirmed removed: secret-file-name') -match 'secret-file-name') { throw 'Snapshot cleanup failure leaked collector output.' }
if ((Get-WCAmcacheSnapshotFailureMessage 'secret-file-name untrusted diagnostic') -match 'secret-file-name') { throw 'Raw collector output leaked into diagnostics.' }
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
    if ($script:mockSnapshotFailure) { $script:WCAmcacheSnapshotFailure = 'App Launch Cache needs administrator access to read a temporary Windows snapshot.'; return $null }
    $script:result | ConvertTo-Json -Depth 10 -Compress
}
function Get-Item {
    [CmdletBinding()] param([string]$LiteralPath, [string]$Path)
    if ($LiteralPath -like '*Amcache.hve') { return [pscustomobject]@{ Length = 1024 } }
    throw [System.Management.Automation.ItemNotFoundException]::new('Not mounted')
}
try {
    $local = Get-WCAmcacheSnapshotInventory
    $record = $local.entries[0].sample[0]
    if (-not $local -or $local.total -ne 1 -or $record.id -ne 'synthetic' -or $record.name -ne 'app' -or $record.path -ne 'C:\app.exe' -or $record.registryValues.Publisher -ne 'synthetic') { throw 'A valid local snapshot lost its identity or details.' }
    # Exercise the JSON boundary as well as the intermediate hashtable.
    $wire = ($local | ConvertTo-Json -Depth 10 -Compress) | ConvertFrom-Json
    $wireRecord = $wire.entries[0].sample[0]
    if ($wireRecord.id -ne 'synthetic' -or $wireRecord.name -ne 'app' -or $wireRecord.path -ne 'C:\app.exe' -or $wireRecord.registryValues.Publisher -ne 'synthetic') { throw 'Snapshot record details were lost during JSON serialization.' }
    $fleet = Get-WCAmcacheSnapshotInventory -FleetProjection
    if (-not $fleet -or $fleet.entries[0].sample[0].ContainsKey('registryValues')) { throw 'Fleet received local-only registry values.' }
    $scan = Get-AmcacheEntries
    if ($scan.status -ne 'complete' -or $scan.source -ne 'snapshot' -or $scan.total -ne 1 -or -not $scan.hveFileExists) { throw 'The cleanup scan did not use the verified snapshot.' }
    $script:mockSnapshotFailure = $true
    $denied = Get-AmcacheEntries
    if (-not $denied.partial -or $denied.message -notmatch 'administrator access' -or $null -ne $denied.total) { throw 'The snapshot failure was not reported accurately.' }
    $script:mockSnapshotFailure = $false
    $originalSamples = $script:result.entries[0].sample
    $script:result.entries[0].sample = @(1..3608 | ForEach-Object { @{ id = "id-$_"; name = "app-$_"; path = ''; registryValues = @{ Publisher = 'synthetic' } } })
    $script:result.entries[0].count = 3608; $script:result.total = 3608
    $large = Get-WCAmcacheSnapshotInventory
    if ($large.total -ne 3608 -or $large.entries[0].sample.Count -ne 3608 -or $large.entries[0].sample[3607].id -ne 'id-3608') { throw 'The large inventory lost records or identifiers.' }
    $script:result.entries[0].sample = $originalSamples; $script:result.entries[0].count = 1; $script:result.total = 1
    $script:result.total = 2
    if ($null -ne (Get-WCAmcacheSnapshotInventory)) { throw 'An inconsistent count was accepted.' }
    if (-not (Get-AmcacheEntries).partial) { throw 'An invalid snapshot did not fall back to unavailable.' }
    $script:result.total = 1; $script:result.status = 'partial'
    if ($null -ne (Get-WCAmcacheSnapshotInventory)) { throw 'A partial snapshot was accepted as complete.' }
    $script:result.status = 'complete'; $script:result.entries = @($script:result.entries | Select-Object -First 4)
    if ($null -ne (Get-WCAmcacheSnapshotInventory)) { throw 'A missing category was accepted.' }
} finally { $env:WINCMD_EXE_PATH = $original; $env:WINCMD_AMCACHE_SNAPSHOT_AVAILABLE = $originalAvailable }
Write-Output 'Passed: complete snapshot, Fleet minimization, and invalid result rejection.'
