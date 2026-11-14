$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $path), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup source did not parse.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-AmcacheEntries' }, $true)
if (-not $definition) { throw 'App Launch Cache reader is missing.' }
. ([scriptblock]::Create($definition.Extent.Text))

$script:denyCategory = $false; $script:mountMissing = $false; $script:availableMount = 'HKLM:\Amcache\Root'; $script:denyHiveFile = $false
$script:childCount = 3
$script:recordMode = 'normal'
$script:denyNativeRoot = $false; $script:emptyLegacy = $false
$script:missingHive = $false; $script:denyHiveParent = $false
$script:missingRecord = $false
$script:recordMode = 'normal'
function Get-Item {
    [CmdletBinding()] param([string]$LiteralPath, [string]$Path)
    if ($LiteralPath -like '*Amcache.hve') {
        if ($script:denyHiveFile) { throw [UnauthorizedAccessException]::new('Denied hive file') }
        if ($script:missingHive) { throw [Management.Automation.ItemNotFoundException]::new('File not found') }
        return [pscustomobject]@{ Length = 0 }
    }
    if ($script:denyNativeRoot -and $Path -eq 'HKLM:\Amcache\Root') { throw [UnauthorizedAccessException]::new('Denied native inventory') }
    if ($script:emptyLegacy -and $Path -like 'HKLM:\AppCompatCache\Amcache\Inventory*') { throw [System.Management.Automation.ItemNotFoundException]::new('Placeholder has no inventory categories') }
    if ($Path -in @('HKLM:\Amcache\Root', 'HKLM:\AppCompatCache\Amcache\Root', 'HKLM:\AppCompatCache\Amcache') -and ($script:mountMissing -or $Path -ne $script:availableMount)) { throw [System.Management.Automation.ItemNotFoundException]::new('Mount unavailable') }
    return [pscustomobject]@{ Name = 'mock category' }
}
function Get-ChildItem {
    [CmdletBinding()] param([string]$Path, [string]$LiteralPath, [switch]$Force)
    if ($LiteralPath) {
        if ($script:denyHiveParent) { throw [UnauthorizedAccessException]::new('Denied hive directory') }
        return
    }
    if ($script:denyCategory -and $Path -like '*InventoryApplication') { throw [UnauthorizedAccessException]::new('Denied') }
    if ($script:childCount -gt 0) { 1..$script:childCount | ForEach-Object { [pscustomobject]@{ PSPath = "mock$_"; PSChildName = "id$_" } } }
}
function Get-ItemProperty {
    [CmdletBinding()] param([Parameter(Position=0)][string]$Path)
    if ($script:missingRecord -and $Path -eq 'mock2') { throw [System.Management.Automation.ItemNotFoundException]::new('Record disappeared') }
    if ($script:recordMode -eq 'hash-only') { return [pscustomobject]@{ Name = ''; ProductName = ''; FileName = ''; BinFileVersion = '1.2.3'; LowerCaseLongPath = ''; LongPath = ''; LongPathHash = 'abc123' } }
    if ($script:recordMode -eq 'fallback') { return [pscustomobject]@{ Name = ''; ProductName = ''; FileName = 'example.exe'; BinFileVersion = ''; LowerCaseLongPath = ''; LongPath = 'C:\Mock\real.exe'; LongPathHash = '' } }
    [pscustomobject]@{ Name = ''; ProductName = 'Example'; BinFileVersion = ''; LowerCaseLongPath = 'C:\Mock\example.exe'; LongPathHash = ''; Publisher = 'Synthetic publisher' }
}

$result = Get-AmcacheEntries
if ($result.error -or $result.total -ne 15 -or @($result.entries).Count -ne 5) { throw 'The five category counts did not total 15.' }
if ($result.inventoryRoot -ne 'HKLM:\Amcache\Root') { throw 'The already-mounted native hive root was not used.' }
if ($result.coverage -ne 'limited' -or $result.coverageNote -notmatch 'offline Amcache file is not read') { throw 'The mounted-only scan must explain its limitations.' }
if (-not $result.hveFileExists -or $result.hveFileSizeMb -ne 0) { throw 'A zero-byte existing hive was reported as absent.' }
if ($result.entries[0].sample[0].name -ne 'Example' -or $result.entries[0].sample[0].path -ne 'C:\Mock\example.exe') { throw 'Sample names or paths were not preserved.' }
if ($result.entries[0].sample[0].registryValues.Publisher -ne 'Synthetic publisher') { throw 'An additional captured Amcache field was discarded.' }
$script:recordMode = 'hash-only'
$hashOnly = (Get-AmcacheEntries).entries[0].sample[0]
if ($hashOnly.name -ne '' -or $hashOnly.path -ne '' -or $hashOnly.registryValues.BinFileVersion -ne '1.2.3' -or $hashOnly.registryValues.LongPathHash -ne 'abc123') { throw 'A version or hash was mislabeled as a name or path, or raw values were lost.' }
$hashFleet = (Get-AmcacheEntries -FleetProjection).entries[0].sample[0]
if ($hashFleet.name -ne '' -or $hashFleet.path -ne '' -or $hashFleet.ContainsKey('registryValues')) { throw 'Fleet projection mislabeled fields or exposed raw values.' }
$script:recordMode = 'fallback'
$fallback = (Get-AmcacheEntries).entries[0].sample[0]
if ($fallback.name -ne 'example.exe' -or $fallback.path -ne 'C:\Mock\real.exe') { throw 'Real filename or long-path fallback was lost.' }
$fallbackFleet = (Get-AmcacheEntries -FleetProjection).entries[0].sample[0]
if ($fallbackFleet.name -ne '' -or $fallbackFleet.path -ne '' -or $fallbackFleet.ContainsKey('registryValues')) { throw 'New local-only fallback values widened Fleet output.' }
$script:recordMode = 'normal'
foreach ($count in @(0, 1)) {
    $script:childCount = $count
    $single = Get-AmcacheEntries
    if ($single.error -or $single.entries[0].sample -isnot [array] -or $single.entries[0].sample.Count -ne $count) { throw "Amcache sample must remain an array with $count entries." }
}
$script:childCount = 3
$fleet = Get-AmcacheEntries -FleetProjection
if ($fleet.entries[0].sample[0].ContainsKey('registryValues')) { throw 'Local-only Amcache values reached the Fleet projection.' }
$script:mountMissing = $true
$unmounted = Get-AmcacheEntries
if (-not $unmounted.partial -or $unmounted.status -ne 'unavailable' -or $null -ne $unmounted.total -or -not $unmounted.hveFileExists -or $unmounted.message -notmatch 'cannot read the offline Amcache file') { throw 'An existing but unmounted hive was reported as complete.' }
$script:missingHive = $true; $script:denyHiveParent = $true
$hiddenHive = Get-AmcacheEntries
if (-not $hiddenHive.partial -or $null -ne $hiddenHive.hveFileExists) { throw 'An inaccessible hive directory must not be treated as a missing hive.' }
$script:denyHiveParent = $false
$absentHive = Get-AmcacheEntries
if ($absentHive.partial -or $absentHive.hveFileExists -ne $false) { throw 'A readable directory without a hive should report an absent hive.' }
$script:missingHive = $false
$script:mountMissing = $false
$script:availableMount = 'HKLM:\AppCompatCache\Amcache'
if ((Get-AmcacheEntries).total -ne 15) { throw 'The existing alternate mount was no longer supported.' }
$script:denyHiveFile = $true
$readableRegistry = Get-AmcacheEntries
if ($readableRegistry.partial -or $readableRegistry.total -ne 15 -or $null -ne $readableRegistry.hveFileExists) { throw 'A readable mounted inventory was blocked by unavailable file metadata.' }
$script:mountMissing = $true
$deniedFile = Get-AmcacheEntries
if (-not $deniedFile.partial -or $deniedFile.status -ne 'unavailable' -or $deniedFile.message -notmatch 'administrator' -or $null -ne $deniedFile.total) { throw 'An inaccessible hive was presented as an empty complete inventory.' }
$script:mountMissing = $false; $script:denyHiveFile = $false
$script:denyCategory = $true
$denied = Get-AmcacheEntries
if (-not $denied.error) { throw 'A denied category read was reported as a successful scan.' }
$script:denyCategory = $false; $script:denyNativeRoot = $true; $script:emptyLegacy = $true
$placeholder = Get-AmcacheEntries
if (-not $placeholder.partial -or $placeholder.status -ne 'unavailable' -or $placeholder.inventoryRoot -or $placeholder.message -notmatch 'administrator') { throw 'An empty fallback mount hid denied native inventory and appeared complete.' }
$script:emptyLegacy = $false
$recognizedFallback = Get-AmcacheEntries
if ($recognizedFallback.partial -or $recognizedFallback.total -ne 15 -or $recognizedFallback.inventoryRoot -ne 'HKLM:\AppCompatCache\Amcache') { throw 'A fallback with real inventory categories was not accepted.' }
$script:denyNativeRoot = $false; $script:missingRecord = $true
$vanished = Get-AmcacheEntries
if (-not $vanished.error) { throw 'A record disappearing while it was read was silently reported as a complete inventory.' }
Write-Output 'Passed: totals, samples, zero-byte existence and denied-read behavior.'
