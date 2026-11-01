$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module parse failed.' }
$definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-StickyNotesInfo' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$script:scenario = 'empty'
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:scenario -eq 'denied') { throw [UnauthorizedAccessException]::new('Synthetic denial') }
    if ($script:scenario -eq 'empty') { throw [Management.Automation.ItemNotFoundException]::new('Synthetic absence') }
    if ($LiteralPath -like '*Packages') { return [pscustomobject]@{ PSIsContainer=$true } }
    if ($LiteralPath -like '*plum.sqlite-wal') { throw [UnauthorizedAccessException]::new('Synthetic denial') }
    if ($LiteralPath -like '*plum.sqlite-shm') { throw [Management.Automation.ItemNotFoundException]::new('Synthetic absence') }
    return [pscustomobject]@{ PSIsContainer=$false; FullName=$LiteralPath; Length=1024; LastWriteTime=[datetime]'2026-01-01' }
}
function Get-ChildItem {
    param([string]$LiteralPath, [switch]$Directory, [string]$Filter, [switch]$Force, $ErrorAction)
    [pscustomobject]@{ FullName="$LiteralPath\Microsoft.MicrosoftStickyNotes_test" }
}
$script:scenario = 'empty'; $empty = Get-StickyNotesInfo
if ($empty.partial -or $empty.total -ne 0) { throw 'Absent package root should be an empty result.' }
$script:scenario = 'denied'; $denied = Get-StickyNotesInfo
if (-not $denied.partial -or $denied.status -ne 'partial') { throw 'Denied package root should be incomplete.' }
$script:scenario = 'partial'; $partial = Get-StickyNotesInfo
if (-not $partial.partial -or $partial.total -ne 1 -or $partial.files[0].path -notlike '*plum.sqlite') { throw 'Available database path or partial state was lost.' }
Write-Output 'Passed: Sticky Notes scans preserve available paths and report incomplete sources.'
