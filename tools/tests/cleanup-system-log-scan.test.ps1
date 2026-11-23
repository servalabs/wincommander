$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
foreach ($helperName in @('Test-CleanupSystemOwnedPath', 'Get-CleanupScanFailureKind', 'Add-CleanupPrivateCoverage')) { . ([scriptblock]::Create($ast.Find({ param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $helperName }, $true).Extent.Text)) }
if ($errors.Count) { throw 'Cleanup module parse failed.' }
foreach ($name in @('Get-CleanupCacheFileInventory','Get-CleanupMultiSourceFileInventory','Get-ServicingLogsInfo','Get-DeviceInstallLogsInfo','Get-UsageTraceLogsInfo')) {
    $definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    . ([scriptblock]::Create($definition.Extent.Text))
}
$script:scenario = 'empty'
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:scenario -eq 'empty') { throw [System.Management.Automation.ItemNotFoundException]::new('Synthetic absent path') }
    if ($script:scenario -eq 'denied' -or ($script:scenario -eq 'partial' -and $LiteralPath -match '(?i)DISM|WDI')) { throw [UnauthorizedAccessException]::new('Synthetic denial') }
    [pscustomobject]@{ PSIsContainer=$true }
}
function Get-ChildItem {
    param([string]$LiteralPath, [string]$Filter, [switch]$File, [switch]$Recurse, [switch]$Force, $ErrorAction)
    if ($script:scenario -eq 'partial' -and $LiteralPath -match '(?i)INF') { throw [UnauthorizedAccessException]::new('Synthetic denied enumeration') }
    [pscustomobject]@{ Name='one.log'; FullName="$LiteralPath\one.log"; Length=1024; LastWriteTime=[datetime]'2026-01-01' }
}
foreach ($fn in @('Get-ServicingLogsInfo','Get-DeviceInstallLogsInfo','Get-UsageTraceLogsInfo')) {
    $script:scenario = 'empty'; $result = & $fn
    if ($result.partial -or $result.total -ne 0) { throw "$fn failed empty inventory" }
    $script:scenario = 'denied'; $result = & $fn
    if ($result.partial -or $result.status -ne 'complete' -or $result.coverage -ne 'limited') { throw "$fn hid denied root" }
    $script:scenario = 'partial'; $result = & $fn
    if ($result.partial -or $result.coverage -ne 'limited') { throw "$fn did not report a limited inventory" }
    if ($fn -ne 'Get-DeviceInstallLogsInfo' -and ($result.total -lt 1 -or -not $result.files[0].path -or -not $result.files[0].source)) { throw "$fn lost available source or path" }
}
Write-Output 'Passed: system log scans retain available paths and report incomplete sources.'
