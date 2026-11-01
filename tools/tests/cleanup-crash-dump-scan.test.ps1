$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module parse failed.' }
$definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-CrashDumpList' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$script:scenario = 'empty'
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:scenario -ne 'empty' -and $LiteralPath -like '*\MEMORY.DMP') {
        return [pscustomobject]@{ PSIsContainer=$false; Name='MEMORY.DMP'; FullName=$LiteralPath; Length=1024; LastWriteTime=[datetime]'2026-01-01' }
    }
    if ($script:scenario -eq 'partial' -and $LiteralPath -like '*\Minidump') { throw [UnauthorizedAccessException]::new('Synthetic denial.') }
    if ($script:scenario -eq 'stream' -and $LiteralPath -like '*\CrashDumps') { return [pscustomobject]@{ PSIsContainer=$true } }
    throw [System.Management.Automation.ItemNotFoundException]::new('Synthetic absent path.')
}
function Get-ChildItem {
    param([string]$LiteralPath, [switch]$Recurse, [switch]$File, [switch]$Force, $ErrorAction)
    [pscustomobject]@{ Name='sample.dmp'; FullName="$LiteralPath\sample.dmp"; Length=2048; LastWriteTime=[datetime]'2026-01-01' }
    throw [UnauthorizedAccessException]::new('Synthetic enumeration denial.')
}
$result = Get-CrashDumpList
if ($result.partial -or $result.total -ne 0) { throw 'Absent sources should produce a complete empty inventory.' }
$script:scenario = 'partial'; $result = Get-CrashDumpList
if (-not $result.partial -or $result.status -ne 'partial' -or $result.total -ne 1 -or $result.dumps[0].path -notlike '*\MEMORY.DMP') { throw 'A denied source must preserve a found dump and mark the inventory incomplete.' }
$script:scenario = 'stream'; $result = Get-CrashDumpList
if (-not $result.partial -or $result.total -ne 2 -or $result.dumps[1].path -notlike '*\sample.dmp') { throw 'An enumeration failure must retain records already emitted.' }
Write-Output 'Passed: crash dump scans retain paths and report incomplete sources.'
