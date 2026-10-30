$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module parse failed.' }
foreach ($name in @('Get-CleanupRecursiveFileInventory', 'Get-P2PUpdateCacheInfo', 'Get-ReliabilityHistoryInfo')) {
    $definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    . ([scriptblock]::Create($definition.Extent.Text))
}
$script:scenario = 'empty'
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:scenario -eq 'empty') { throw [Management.Automation.ItemNotFoundException]::new('Synthetic missing root') }
    if ($script:scenario -eq 'denied') { throw [UnauthorizedAccessException]::new('Synthetic denial') }
    [pscustomobject]@{ PSIsContainer=$true }
}
function Get-ChildItem {
    param([string]$LiteralPath, [switch]$Recurse, [switch]$File, [switch]$Force, $ErrorAction)
    [pscustomobject]@{ Name='one.dat'; FullName="$LiteralPath\one.dat"; Length=1024; LastWriteTime=[datetime]'2026-01-01' }
    if ($script:scenario -eq 'partial') { throw [UnauthorizedAccessException]::new('Synthetic enumeration denial') }
}
foreach ($fn in @('Get-P2PUpdateCacheInfo', 'Get-ReliabilityHistoryInfo')) {
    $script:scenario = 'empty'; $result = & $fn
    if ($result.partial -or $result.total -ne 0) { throw "$fn failed empty inventory" }
    $script:scenario = 'denied'; $result = & $fn
    if (-not $result.partial -or $result.total -ne 0) { throw "$fn hid denied root" }
    $script:scenario = 'partial'; $result = & $fn
    if (-not $result.partial -or $result.total -ne 1 -or -not $result.files[0].path -or -not $result.files[0].source) { throw "$fn lost the observed file or hid incomplete enumeration" }
    $script:scenario = 'complete'; $result = & $fn
    if ($result.partial -or $result.total -ne 1) { throw "$fn failed complete inventory" }
}
Write-Output 'Passed: recursive scans preserve paths and report partial access.'
