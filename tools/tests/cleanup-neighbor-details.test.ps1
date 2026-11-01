$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-NeighborCacheInfo' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
function Get-NetNeighbor {
    param($ErrorAction)
    [pscustomobject]@{ InterfaceAlias='Fixture'; InterfaceIndex=7; IPAddress='192.0.2.10'; LinkLayerAddress='00-11-22-33-44-55'; State='Reachable'; AddressFamily='IPv4' }
    [pscustomobject]@{ InterfaceAlias='Fixture'; InterfaceIndex=7; IPAddress='192.0.2.11'; LinkLayerAddress='00-11-22-33-44-66'; State='Permanent'; AddressFamily='IPv4' }
    if ($script:failNeighbor) { throw 'Fixture provider failure' }
}
$result = Get-NeighborCacheInfo
if ($result.partial -or $result.total -ne 1 -or $result.files[0].address -ne '192.0.2.10' -or $result.files[0].linkLayerAddress -ne '00-11-22-33-44-55' -or $result.files[0].interfaceIndex -ne 7) { throw 'Neighbor details or cleanup scope were not retained.' }
$script:failNeighbor = $true
$partial = Get-NeighborCacheInfo
if (-not $partial.partial -or $partial.total -ne 1 -or $partial.files[0].address -ne '192.0.2.10') { throw 'Partial provider failure discarded observed neighbors.' }
$fleet = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-FleetForensicProjection' }, $true)
if ($fleet.Extent.Text -notmatch '''neighbor_cache''\s*\{[^\r\n]*\$result\.error -or \$result\.partial') { throw 'Fleet must report a partial neighbor scan as unavailable.' }
Write-Output 'Passed: neighbor details and partial scan behavior.'
