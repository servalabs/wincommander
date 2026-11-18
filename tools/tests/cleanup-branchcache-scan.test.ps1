$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module parse failed.' }
$fn = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-BranchCacheInfo' }, $true)
. ([scriptblock]::Create($fn.Extent.Text))
$script:mode = 'missing'
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:mode -eq 'missing') { throw [System.Management.Automation.ItemNotFoundException]::new('absent') }
    if ($script:mode -eq 'denied' -or ($script:mode -eq 'partial' -and $LiteralPath -match 'PeerDistPub')) { throw [UnauthorizedAccessException]::new('denied') }
    [pscustomobject]@{ PSIsContainer=$true }
}
function Get-ChildItem {
    param([string]$LiteralPath, [switch]$Recurse, [switch]$File, [switch]$Force, $ErrorAction)
    [pscustomobject]@{ Name='sample.bin'; FullName="$LiteralPath\sample.bin"; Length=1024; LastWriteTime=[datetime]'2026-01-01' }
    if ($script:mode -eq 'stream') { throw [UnauthorizedAccessException]::new('denied after record') }
}
$r=Get-BranchCacheInfo
if ($r.partial -or $r.total -ne 0) { throw 'Absent folders should be empty.' }
$script:mode='denied'; $r=Get-BranchCacheInfo
if (-not $r.partial -or $r.total -ne 0) { throw 'Denied sources must be incomplete.' }
$script:mode='partial'; $r=Get-BranchCacheInfo
if (-not $r.partial -or $r.total -ne 1 -or $r.files[0].source -ne 'PeerDistRep' -or -not $r.files[0].path.EndsWith('sample.bin')) { throw 'Accessible record or path was lost.' }
$script:mode='stream'; $r=Get-BranchCacheInfo
if (-not $r.partial -or $r.total -ne 2) { throw 'Records produced before source errors were lost.' }
$fleetFn = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-FleetForensicProjection' }, $true)
. ([scriptblock]::Create($fleetFn.Extent.Text))
function Get-BranchCacheInfo { @{ partial = $true; files = @(); total = 0 } }
$projection = Get-FleetForensicProjection -Category 'peer_distribution_cache'
if (-not $projection.error -or $projection.total -ne 0) { throw 'Fleet presented a partial scan as a complete inventory.' }
Write-Output 'Passed: BranchCache retains paths and reports incomplete sources.'
