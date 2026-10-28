$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')
$parsed = [Management.Automation.Language.Parser]::ParseInput($script:EraseFunctions, [ref]$null, [ref]$null)
foreach ($name in @('Erase-OneFile','Erase-Dir','Get-AutoEraseFiles','Get-AutoEraseDirectories')) {
    $node = $parsed.Find({ param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name }, $true)
    Invoke-Expression $node.Extent.Text
}
$script:AutoEraseRemoved = 0; $script:AutoEraseFailed = 0; $script:AutoEraseDeferred = 0
$script:renameCalls = 0; $script:removeCalls = 0; $script:rootAttributes = 0; $script:childKind = ''; $script:asDirectory = $false
function Get-Item { [CmdletBinding()] param($LiteralPath,[switch]$Force) [pscustomobject]@{ FullName=$LiteralPath; Length=16; PSIsContainer=$script:asDirectory; Attributes=$script:rootAttributes } }
function Set-ItemProperty { [CmdletBinding()] param($LiteralPath,$Name,$Value) }
function Rename-Item { [CmdletBinding()] param($LiteralPath,$NewName,[switch]$Force) $script:renameCalls++ }
function Remove-Item { [CmdletBinding()] param($LiteralPath,[switch]$Force,[switch]$Recurse) $script:removeCalls++ }
function Get-ChildItem {
    [CmdletBinding()] param($LiteralPath,$Filter,[switch]$Force,[switch]$File,[switch]$Directory,[switch]$Recurse)
    if ($script:childKind -eq 'denied') { throw [UnauthorizedAccessException]::new('Synthetic denied traversal') }
    if (($script:childKind -eq 'file' -and $File) -or ($script:childKind -eq 'directory' -and $Directory)) {
        [pscustomobject]@{ FullName=(Join-Path $LiteralPath 'synthetic-link'); Attributes=[IO.FileAttributes]::ReparsePoint }
    }
}
# Get-Item is mocked as populated, while this unique nonexistent path makes the actual stream open fail.
$syntheticMissingPath = Join-Path ([IO.Path]::GetTempPath()) ('NeverCreated-' + [Guid]::NewGuid().ToString('N'))
if (Erase-OneFile $syntheticMissingPath) { throw 'Failed overwrite reported success.' }
if ($script:AutoEraseFailed -ne 1 -or $script:renameCalls -or $script:removeCalls) { throw 'Failed overwrite proceeded to rename or delete.' }

$script:asDirectory = $true
foreach ($kind in @('root','file','directory','denied')) {
    $script:rootAttributes = if ($kind -eq 'root') { [IO.FileAttributes]::ReparsePoint } else { 0 }
    $script:childKind = $kind; $caught = $false
    try { $null = @(Get-AutoEraseFiles -Path $syntheticMissingPath -Recurse) } catch { $caught = $true }
    if (-not $caught) { throw "Traversal did not fail closed for $kind" }
    $before = $script:AutoEraseFailed
    Erase-Dir $syntheticMissingPath
    if ($script:AutoEraseFailed -le $before -or $script:removeCalls -or $script:renameCalls) { throw "Directory erase mutated files after $kind traversal failure." }
}
Write-Output 'Passed: overwrite failure cannot rename/delete; reparse roots/files/directories and denied traversal fail closed.'
