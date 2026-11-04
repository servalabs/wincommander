$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-SRUMData' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
function Get-Item {
    param($LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:mode -eq 'absent') { throw [Management.Automation.ItemNotFoundException]::new('Missing directory.') }
    [pscustomobject]@{ PSIsContainer=$true; Attributes=[IO.FileAttributes]::Directory }
}
function Get-ChildItem {
    param($LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:mode -eq 'denied') { throw [UnauthorizedAccessException]::new('Directory access denied.') }
    if ($script:mode -eq 'present') { [pscustomobject]@{ Name='SRUDB.dat'; PSIsContainer=$false; Length=1024; LastWriteTime=[datetime]'2026-01-01' } }
}
$script:mode='absent'; $absent=Get-SRUMData
if ($absent.error -or $absent.databaseExists -ne $false) { throw 'An absent directory should report no database.' }
$script:mode='denied'; $denied=Get-SRUMData
if (-not $denied.error) { throw 'An unreadable directory must not report an absent database.' }
$script:mode='present'; $present=Get-SRUMData
if ($present.error -or -not $present.databaseExists -or $present.total -ne 1) { throw 'A readable database was not reported.' }
Write-Output 'Passed: absent, inaccessible and readable SRUM locations.'
