$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'The cleanup module could not be parsed.' }
$definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-DockerDesktopDataInfo' }, $true)
if (-not $definition) { throw 'The Docker inventory is unavailable.' }
. ([scriptblock]::Create($definition.Extent.Text))

# No real Docker settings, disks or logs are accessed by these provider stubs.
$script:scenario = 'missing'
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, $ErrorAction)
    $isRoot = $LiteralPath -like '*Docker\wsl'
    $isFixedDisk = $LiteralPath -like '*DockerDesktop.vhdx'
    if ($script:scenario -eq 'missing' -or ($script:scenario -eq 'wsl-only' -and -not $isRoot)) { throw [System.Management.Automation.ItemNotFoundException]::new('Synthetic absent path.') }
    if ($script:scenario -eq 'fixed-disks' -and -not $isFixedDisk) { throw [System.Management.Automation.ItemNotFoundException]::new('Synthetic absent path.') }
    if ($script:scenario -eq 'denied-fixed' -and $isFixedDisk) { throw [UnauthorizedAccessException]::new('Synthetic denial.') }
    $attributes = if ($script:scenario -eq 'reparse-fixed' -and $isFixedDisk) { [IO.FileAttributes]::ReparsePoint } else { [IO.FileAttributes]::Normal }
    [pscustomobject]@{ FullName=$LiteralPath; Name=[IO.Path]::GetFileName($LiteralPath); PSIsContainer=$isRoot; Attributes=$attributes; Length=1024; LastWriteTime=[datetime]'2026-01-01' }
}
function Get-ChildItem {
    param([string]$LiteralPath, [switch]$Recurse, [switch]$Force, $ErrorAction)
    if ($script:scenario -eq 'denied-wsl') { throw [UnauthorizedAccessException]::new('Synthetic denial.') }
    if ($script:scenario -eq 'reparse-entry') { return [pscustomobject]@{ Name='link'; PSIsContainer=$true; Attributes=[IO.FileAttributes]::ReparsePoint } }
    if ($script:scenario -eq 'wsl-only') { return [pscustomobject]@{ Name='data.vhdx'; FullName="$LiteralPath\disk\data.vhdx"; PSIsContainer=$false; Attributes=[IO.FileAttributes]::Normal; Length=2048; LastWriteTime=[datetime]'2026-01-01' } }
    return @()
}

$result = Get-DockerDesktopDataInfo
if ($result.error -or $result.total -ne 0 -or $result.coverage -ne 'limited') { throw 'Missing default paths must be an explicitly limited empty result.' }
$script:scenario = 'fixed-disks'; $result = Get-DockerDesktopDataInfo
if ($result.error -or $result.total -ne 2 -or @($result.files | Where-Object { -not $_.path }).Count) { throw 'Documented Hyper-V/VMM disks and paths must be returned.' }
$script:scenario = 'wsl-only'; $result = Get-DockerDesktopDataInfo
if ($result.error -or $result.total -ne 1 -or $result.files[0].path -notlike '*\disk\data.vhdx') { throw 'Default WSL disk paths must be returned.' }
foreach ($scenario in @('denied-fixed','denied-wsl','reparse-fixed','reparse-entry')) {
    $script:scenario = $scenario; $result = Get-DockerDesktopDataInfo
    if (-not $result.error) { throw "Docker inventory must not claim success for $scenario." }
}
Write-Output 'Passed: Docker default WSL, Hyper-V and VMM inventory with limited coverage and failure handling.'
