$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module parse failed.' }
$definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-EventTranscriptInfo' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$script:scenario = 'empty'
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:scenario -eq 'empty') { throw [Management.Automation.ItemNotFoundException]::new('Synthetic missing directory') }
    if ($script:scenario -eq 'denied') { throw [UnauthorizedAccessException]::new('Synthetic denial') }
    [pscustomobject]@{ PSIsContainer=$true }
}
function Get-ChildItem {
    param([string]$LiteralPath, [string]$Filter, [switch]$Recurse, [switch]$File, [switch]$Force, $ErrorAction)
    if ($Filter -notin @('EventTranscript.db', '*.etl')) { return }
    [pscustomobject]@{ Name='synthetic.dat'; FullName="$LiteralPath\synthetic.dat"; Length=1024; LastWriteTime=[datetime]'2026-01-01' }
    if ($script:scenario -eq 'partial' -and $Filter -eq 'EventTranscript.db') { throw [UnauthorizedAccessException]::new('Synthetic enumeration denial') }
}
$script:scenario = 'empty'; $result = Get-EventTranscriptInfo
if ($result.partial -or $result.total -ne 0) { throw 'Missing directories were not handled.' }
$script:scenario = 'denied'; $result = Get-EventTranscriptInfo
if (-not $result.partial -or $result.total -ne 0) { throw 'Denied directories were reported as complete.' }
$script:scenario = 'partial'; $result = Get-EventTranscriptInfo
if (-not $result.partial -or $result.total -ne 2 -or -not $result.files[0].path -or -not $result.files[1].path) { throw 'Partial observations were lost.' }
$script:scenario = 'complete'; $result = Get-EventTranscriptInfo
if ($result.partial -or $result.total -ne 2) { throw 'Complete inventory was not returned.' }
Write-Output 'Passed: diagnostic scan retains paths and partial observations.'
