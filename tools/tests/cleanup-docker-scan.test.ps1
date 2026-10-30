$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module parse failed.' }
$definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-DockerDesktopDataInfo' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$script:scenario = 'empty'
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:scenario -eq 'stream' -and $LiteralPath -like '*Docker\wsl') { return [pscustomobject]@{ PSIsContainer=$true; Attributes=[IO.FileAttributes]::Directory } }
    if ($script:scenario -eq 'partial' -and $LiteralPath -like '*DockerDesktop\vm-data\DockerDesktop.vhdx') { throw [UnauthorizedAccessException]::new('Synthetic denial.') }
    if ($script:scenario -eq 'partial' -and $LiteralPath -like '*Docker\log.txt') {
        return [pscustomobject]@{ PSIsContainer=$false; Attributes=[IO.FileAttributes]::Normal; FullName=$LiteralPath; Length=1024; LastWriteTime=[datetime]'2026-01-01' }
    }
    throw [System.Management.Automation.ItemNotFoundException]::new('Synthetic absent path.')
}
function Get-ChildItem {
    param([string]$LiteralPath, [switch]$Recurse, [switch]$Force, $ErrorAction)
    [pscustomobject]@{ Name='disk.vhdx'; FullName="$LiteralPath\disk.vhdx"; PSIsContainer=$false; Attributes=[IO.FileAttributes]::Normal; Length=1024; LastWriteTime=[datetime]'2026-01-01' }
    throw [UnauthorizedAccessException]::new('Synthetic enumeration denial.')
}
$result = Get-DockerDesktopDataInfo
if ($result.partial -or $result.total -ne 0 -or $result.coverage -ne 'limited') { throw 'An accessible empty default inventory should retain its limited coverage.' }
$script:scenario = 'partial'; $result = Get-DockerDesktopDataInfo
if (-not $result.partial -or $result.status -ne 'partial' -or $result.total -ne 1 -or $result.files[0].name -ne 'Docker: log.txt') { throw 'A denied source must preserve captured rows and report a partial inventory.' }
$script:scenario = 'stream'; $result = Get-DockerDesktopDataInfo
if (-not $result.partial -or $result.total -ne 1 -or $result.files[0].name -ne 'Docker WSL: disk.vhdx') { throw 'Enumeration failures must preserve records already emitted by the same source.' }
Write-Output 'Passed: Docker scan preserves captured rows and marks inaccessible sources incomplete.'
