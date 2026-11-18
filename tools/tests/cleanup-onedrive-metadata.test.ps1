$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module parse failed.' }
$fn = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-OneDriveMetadataInfo' }, $true)
. ([scriptblock]::Create($fn.Extent.Text))
$script:mode = 'missing'
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:mode -eq 'missing') { throw [Management.Automation.ItemNotFoundException]::new('missing') }
    if ($script:mode -eq 'denied') { throw [UnauthorizedAccessException]::new('denied') }
    [pscustomobject]@{ PSIsContainer=$true; FullName=$LiteralPath }
}
function Get-ChildItem {
    param([string]$LiteralPath, [switch]$Directory, [switch]$Recurse, [switch]$File, [string]$Filter, [switch]$Force, $ErrorAction)
    if ($Directory) { [pscustomobject]@{ FullName="$LiteralPath\account" }; return }
    if ($Filter -eq '*.dat') {
        [pscustomobject]@{ FullName="$LiteralPath\sample.dat"; Length=1024; LastWriteTime=[datetime]'2026-01-01' }
        if ($script:mode -eq 'stream') { throw [UnauthorizedAccessException]::new('denied after record') }
    } elseif ($script:mode -eq 'partial') { throw [UnauthorizedAccessException]::new('denied') }
}
$r = Get-OneDriveMetadataInfo
if ($r.partial -or $r.total -ne 0) { throw 'Absent metadata should be empty.' }
$script:mode = 'denied'; $r = Get-OneDriveMetadataInfo
if (-not $r.partial -or $r.total -ne 0) { throw 'Denied metadata should be incomplete.' }
$script:mode = 'partial'; $r = Get-OneDriveMetadataInfo
if (-not $r.partial -or $r.total -ne 1 -or -not $r.files[0].path.EndsWith('sample.dat')) { throw 'Accessible metadata and its path must be preserved.' }
$script:mode = 'stream'; $r = Get-OneDriveMetadataInfo
if (-not $r.partial -or $r.total -ne 1) { throw 'A record emitted before an error must be retained.' }
$script:mode = 'complete'; $r = Get-OneDriveMetadataInfo
if ($r.partial -or $r.total -ne 1) { throw 'Readable metadata should be complete.' }
$fleetFn = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-FleetForensicProjection' }, $true)
. ([scriptblock]::Create($fleetFn.Extent.Text))
function Get-OneDriveMetadataInfo { @{ partial = $true; files = @(); total = 0 } }
$projection = Get-FleetForensicProjection -Category 'onedrive_metadata'
if (-not $projection.error -or $projection.total -ne 0) { throw 'Fleet presented a partial scan as a complete inventory.' }
Write-Output 'Passed: OneDrive metadata retains paths and reports incomplete sources.'
