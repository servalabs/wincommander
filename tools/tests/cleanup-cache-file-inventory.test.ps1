$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module parse failed.' }
foreach ($name in @('Get-CleanupCacheFileInventory','Get-WebCacheInfo','Get-ThumbnailCacheInfo','Get-NotificationDatabaseInfo')) {
    $definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    . ([scriptblock]::Create($definition.Extent.Text))
}
$script:scenario = 'empty'
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:scenario -eq 'denied') { throw [UnauthorizedAccessException]::new('Synthetic denial') }
    if ($script:scenario -eq 'partial' -and $LiteralPath -notlike '*wpnidm') { return [pscustomobject]@{ PSIsContainer=$true } }
    throw [System.Management.Automation.ItemNotFoundException]::new('Synthetic absent path')
}
function Get-ChildItem {
    param([string]$LiteralPath, [string]$Filter, [switch]$File, [switch]$Recurse, [switch]$Force, $ErrorAction)
    if ($Filter -like 'WebCache*' -or $Filter -like 'thumbcache*' -or $Filter -eq 'wpndatabase.db') {
        [pscustomobject]@{ Name='one'; FullName="$LiteralPath\one"; Length=1024; LastWriteTime=[datetime]'2026-01-01' }
    } else { throw [UnauthorizedAccessException]::new('Synthetic partial failure') }
}
foreach ($fn in @('Get-WebCacheInfo','Get-ThumbnailCacheInfo','Get-NotificationDatabaseInfo')) {
    $script:scenario = 'empty'; $result = & $fn
    if ($result.partial -or $result.total -ne 0) { throw "$fn failed empty inventory" }
    $script:scenario = 'denied'; $result = & $fn
    if (-not $result.partial -or $result.status -ne 'partial') { throw "$fn hid denied root" }
    $script:scenario = 'partial'; $result = & $fn
    if (-not $result.partial -or $result.total -ne 1 -or -not $result.files[0].path) { throw "$fn lost available paths or partial status" }
}
Write-Output 'Passed: cache scans preserve paths and report incomplete sources.'
