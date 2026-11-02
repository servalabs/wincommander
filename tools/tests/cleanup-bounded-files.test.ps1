$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw 'Cleanup module did not parse.' }
foreach ($name in @('Get-CleanupBoundedFileInventory', 'Get-SQLiteWALList')) {
    $definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    . ([scriptblock]::Create($definition.Extent.Text))
}
$script:scenario = 'complete'; $script:visited = @()
function Get-Item {
    [CmdletBinding()] param($LiteralPath, [switch]$Force)
    if ($script:scenario -eq 'missing') { throw [Management.Automation.ItemNotFoundException]::new('Synthetic missing root') }
    if ($script:scenario -eq 'denied-root') { throw [UnauthorizedAccessException]::new('Synthetic denied root') }
    [pscustomobject]@{ PSIsContainer = $true; Attributes = [IO.FileAttributes]::Directory }
}
function Get-ChildItem {
    [CmdletBinding()] param($LiteralPath, [switch]$Force)
    $script:visited += $LiteralPath
    if ($LiteralPath -like '*Application Data') { throw 'A compatibility junction was traversed.' }
    if ($LiteralPath -like '*\nested') {
        if ($script:scenario -eq 'denied-child') { throw [UnauthorizedAccessException]::new('Synthetic denied child') }
        [pscustomobject]@{ Name='nested.sqlite-shm'; FullName="$LiteralPath\nested.sqlite-shm"; PSIsContainer=$false; Attributes=[IO.FileAttributes]::Normal; Length=2048; LastWriteTime=[datetime]'2026-01-01' }
        return
    }
    [pscustomobject]@{ Name='Application Data'; FullName="$LiteralPath\Application Data"; PSIsContainer=$true; Attributes=([IO.FileAttributes]::Directory -bor [IO.FileAttributes]::ReparsePoint) }
    [pscustomobject]@{ Name='nested'; FullName="$LiteralPath\nested"; PSIsContainer=$true; Attributes=[IO.FileAttributes]::Directory }
    foreach ($name in @('one.wal','two.db-wal','three.sqlite-wal','four.shm','five.db-shm','unrelated.txt')) {
        [pscustomobject]@{ Name=$name; FullName="$LiteralPath\$name"; PSIsContainer=$false; Attributes=[IO.FileAttributes]::Normal; Length=1024; LastWriteTime=[datetime]'2026-01-01' }
    }
}
$originalLocal = $env:LOCALAPPDATA; $originalRoaming = $env:APPDATA
try {
    $env:LOCALAPPDATA='C:\fixture\Local'; $env:APPDATA='C:\fixture\Roaming'
    $result = Get-SQLiteWALList
    if ($result.error -or $result.partial -or $result.total -ne 12 -or $result.skippedLinks -ne 2) { throw 'Bounded inventory failed to retain all WAL/SHM forms or skip links.' }
    if ($script:visited -match 'Application Data') { throw 'A directory junction was opened.' }
    $shallow = Get-CleanupBoundedFileInventory -Roots @('C:\fixture') -Patterns @('*.wal') -Depth 0
    if ($shallow.files.Count -ne 1 -or $shallow.partial) { throw 'Depth limit was not preserved.' }
    $script:scenario='denied-child'
    $partial = Get-SQLiteWALList
    if (-not $partial.partial -or $partial.error -or $partial.total -ne 10 -or $partial.message -notmatch 'Synthetic denied child') { throw 'A real denied directory lost its observations or was hidden.' }
    $script:scenario='denied-root'
    $denied = Get-SQLiteWALList
    if (-not $denied.error -or -not $denied.partial -or $denied.total -ne 0) { throw 'Denied roots appeared empty and complete.' }
    $script:scenario='missing'
    $missing = Get-SQLiteWALList
    if ($missing.error -or $missing.partial -or $missing.total -ne 0) { throw 'Absent locations appeared denied.' }
    'PASS: bounded WAL scans skip junctions, cover WAL/SHM names, and preserve genuine denied coverage.'
} finally { $env:LOCALAPPDATA=$originalLocal; $env:APPDATA=$originalRoaming }
