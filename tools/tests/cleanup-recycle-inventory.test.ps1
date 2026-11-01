$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module did not parse.' }
$definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-RecycleBinInfo' }, $true)
if (-not $definition) { throw 'Recycle Bin scan is missing.' }
$text = $definition.Extent.Text.Replace('[System.IO.DriveInfo]::GetDrives()', '$(Get-FakeRecycleDrives)').Replace('[System.IO.File]::ReadAllBytes($iFile.FullName)', '$(Read-FakeRecycleBytes)')
. ([scriptblock]::Create($text))
function Get-FakeRecycleDrives { [pscustomobject]@{ DriveType = 'Fixed'; IsReady = $true; Name = 'C:\'; RootDirectory = [pscustomobject]@{ FullName = 'C:\' } } }
function Get-Item {
    param($LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:mode -eq 'absent') { throw [Management.Automation.ItemNotFoundException]::new('Synthetic absent root') }
    if ($script:mode -eq 'denied') { throw [UnauthorizedAccessException]::new('Synthetic denied root') }
    [pscustomobject]@{ PSIsContainer = $true; Attributes = [IO.FileAttributes]::Directory }
}
function Get-ChildItem {
    param($LiteralPath, $Filter, [switch]$Directory, [switch]$File, [switch]$Force, $ErrorAction)
    if ($Directory) { return [pscustomobject]@{ Name = 'invalid-sid'; FullName = 'C:\fixture\user'; Attributes = [IO.FileAttributes]::Directory } }
    if ($script:mode -eq 'enumeration-denied') { throw [UnauthorizedAccessException]::new('Synthetic denied metadata inventory') }
    return [pscustomobject]@{ FullName = 'C:\fixture\user\$Itest'; Length = 32; Attributes = [IO.FileAttributes]::Normal }
}
function Read-FakeRecycleBytes {
    if ($script:mode -eq 'short') { return ,([byte[]]@(1, 2)) }
    $bytes = [byte[]]::new(32)
    [BitConverter]::GetBytes([long]2).CopyTo($bytes, 0)
    [BitConverter]::GetBytes([long]1024).CopyTo($bytes, 8)
    [BitConverter]::GetBytes([int]2).CopyTo($bytes, 24)
    [Text.Encoding]::Unicode.GetBytes('X').CopyTo($bytes, 28)
    return ,$bytes
}
$script:mode = 'valid'
$result = Get-RecycleBinInfo
if ($result.partial -or $result.total -ne 1 -or $result.items[0].originalPath -ne 'X') { throw 'Readable metadata was not reported accurately.' }
foreach ($mode in @('denied', 'enumeration-denied', 'short')) {
    $script:mode = $mode; $result = Get-RecycleBinInfo
    if (-not $result.partial -or $result.status -ne 'partial') { throw "An incomplete scan was reported complete: $mode" }
}
$script:mode = 'absent'; $result = Get-RecycleBinInfo
if ($result.partial -or $result.total -ne 0) { throw 'An absent Recycle Bin was not reported as empty.' }
Write-Output 'Passed: readable, missing, denied and malformed Recycle Bin inventory.'
