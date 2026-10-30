$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module parse failed.' }
foreach ($name in @('Get-CleanupCacheFileInventory','Get-ActivitiesTimelineInfo','Get-RdpBitmapCacheInfo','Get-EmbeddedWebCacheInfo')) {
    $definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    . ([scriptblock]::Create($definition.Extent.Text))
}
$script:scenario = 'empty'
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:scenario -eq 'denied') { throw [UnauthorizedAccessException]::new('Synthetic denial') }
    if ($script:scenario -eq 'partial') { return [pscustomobject]@{ PSIsContainer=$true } }
    throw [System.Management.Automation.ItemNotFoundException]::new('Synthetic absent path')
}
function Get-ChildItem {
    param([string]$LiteralPath, [string[]]$Include, [string]$Filter, [switch]$File, [switch]$Directory, [switch]$Recurse, [switch]$Force, $Depth, $ErrorAction)
    if ($Directory) {
        [pscustomobject]@{ Name='EBWebView'; FullName="$LiteralPath\app\EBWebView"; LastWriteTime=[datetime]'2026-01-01' }
        throw [UnauthorizedAccessException]::new('Synthetic partial failure')
    }
    if ($LiteralPath -like '*EBWebView') { throw [UnauthorizedAccessException]::new('Synthetic inaccessible folder') }
    if ($Filter -eq '*.bmc') { throw [UnauthorizedAccessException]::new('Synthetic partial failure') }
    [pscustomobject]@{ Name='one'; FullName="$LiteralPath\one"; Length=1024; LastWriteTime=[datetime]'2026-01-01' }
    if ($Include) { throw [UnauthorizedAccessException]::new('Synthetic partial failure') }
}
foreach ($fn in @('Get-ActivitiesTimelineInfo','Get-RdpBitmapCacheInfo','Get-EmbeddedWebCacheInfo')) {
    $script:scenario = 'empty'; $result = & $fn
    if ($result.partial -or $result.total -ne 0) { throw "$fn failed empty inventory" }
    $script:scenario = 'denied'; $result = & $fn
    if (-not $result.partial -or $result.status -ne 'partial') { throw "$fn hid denied root" }
    $script:scenario = 'partial'; $result = & $fn
    if (-not $result.partial -or $result.total -ne 1 -or -not $result.files[0].path) { throw "$fn lost available paths or partial status" }
}
Write-Output 'Passed: activity and app cache scans preserve paths and report incomplete sources.'
