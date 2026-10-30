$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module parse failed.' }
foreach ($name in @('Get-CleanupRecursiveFileInventory', 'Get-DefenderHistoryInfo')) {
    $definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    . ([scriptblock]::Create($definition.Extent.Text))
}
$script:scenario = 'missing'
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:scenario -eq 'missing') { throw [Management.Automation.ItemNotFoundException]::new('Synthetic missing') }
    if ($LiteralPath -like '*MpCmdRun.log') {
        if ($script:scenario -eq 'deniedLog') { throw [UnauthorizedAccessException]::new('Synthetic denial') }
        return [pscustomobject]@{ PSIsContainer=$false; Name='MpCmdRun.log'; FullName=$LiteralPath; Length=2048; LastWriteTime=[datetime]'2026-01-01' }
    }
    [pscustomobject]@{ PSIsContainer=$true }
}
function Get-ChildItem {
    param([string]$LiteralPath, [switch]$Recurse, [switch]$File, [switch]$Force, $ErrorAction)
    [pscustomobject]@{ Name='one.dat'; FullName="$LiteralPath\one.dat"; Length=1024; LastWriteTime=[datetime]'2026-01-01' }
    if ($script:scenario -eq 'partial') { throw [UnauthorizedAccessException]::new('Synthetic enumeration denial') }
}
$script:scenario='missing'; $result=Get-DefenderHistoryInfo
if ($result.partial -or $result.total -ne 0) { throw 'Missing sources should be empty.' }
$script:scenario='complete'; $result=Get-DefenderHistoryInfo
if ($result.partial -or $result.total -ne 2 -or @($result.files | Where-Object { -not $_.path }).Count) { throw 'Complete records must retain both paths.' }
$script:scenario='partial'; $result=Get-DefenderHistoryInfo
if (-not $result.partial -or $result.total -ne 2) { throw 'Partial enumeration must retain records.' }
$script:scenario='deniedLog'; $result=Get-DefenderHistoryInfo
if (-not $result.partial -or $result.total -ne 1) { throw 'Denied log must retain scan history.' }
Write-Output 'Passed: protection history retains records and reports incomplete sources.'
