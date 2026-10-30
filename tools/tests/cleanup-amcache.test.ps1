$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $path), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup source did not parse.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-AmcacheEntries' }, $true)
if (-not $definition) { throw 'App Launch Cache reader is missing.' }
. ([scriptblock]::Create($definition.Extent.Text))

$script:denyCategory = $false; $script:mountMissing = $false
function Get-Item {
    [CmdletBinding()] param([string]$LiteralPath, [string]$Path)
    if ($LiteralPath -like '*Amcache.hve') { return [pscustomobject]@{ Length = 0 } }
    if ($script:mountMissing -and $Path -eq 'HKLM:\AppCompatCache\Amcache') { throw [System.Management.Automation.ItemNotFoundException]::new('Mount unavailable') }
    return [pscustomobject]@{ Name = 'mock category' }
}
function Get-ChildItem {
    [CmdletBinding()] param([string]$Path)
    if ($script:denyCategory -and $Path -like '*InventoryApplication') { throw [UnauthorizedAccessException]::new('Denied') }
    1..3 | ForEach-Object { [pscustomobject]@{ PSPath = "mock$_"; PSChildName = "id$_" } }
}
function Get-ItemProperty {
    [CmdletBinding()] param([Parameter(Position=0)][string]$Path)
    [pscustomobject]@{ Name = ''; ProductName = 'Example'; BinFileVersion = ''; LowerCaseLongPath = 'C:\Mock\example.exe'; LongPathHash = ''; Publisher = 'Synthetic publisher' }
}

$result = Get-AmcacheEntries
if ($result.error -or $result.total -ne 15 -or @($result.entries).Count -ne 5) { throw 'The five category counts did not total 15.' }
if (-not $result.hveFileExists -or $result.hveFileSizeMb -ne 0) { throw 'A zero-byte existing hive was reported as absent.' }
if ($result.entries[0].sample[0].name -ne 'Example' -or $result.entries[0].sample[0].path -ne 'C:\Mock\example.exe') { throw 'Sample names or paths were not preserved.' }
if ($result.entries[0].sample[0].registryValues.Publisher -ne 'Synthetic publisher') { throw 'An additional captured Amcache field was discarded.' }
$fleet = Get-AmcacheEntries -FleetProjection
if ($fleet.entries[0].sample[0].ContainsKey('registryValues')) { throw 'Local-only Amcache values reached the Fleet projection.' }
$script:mountMissing = $true
$unmounted = Get-AmcacheEntries
if (-not $unmounted.partial -or $unmounted.status -ne 'partial' -or -not $unmounted.hveFileExists) { throw 'An existing but unmounted hive was reported as complete.' }
$script:mountMissing = $false
$script:denyCategory = $true
$denied = Get-AmcacheEntries
if (-not $denied.error) { throw 'A denied category read was reported as a successful scan.' }
Write-Output 'Passed: totals, samples, zero-byte existence and denied-read behavior.'
