$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module did not parse.' }
$definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-RecycleBinInfo' }, $true)
if (-not $definition) { throw 'Recycle Bin scan is missing.' }
$text = $definition.Extent.Text.Replace('[System.IO.DriveInfo]::GetDrives()', '$(Get-FakeRecycleDrives)').Replace('[System.IO.File]::ReadAllBytes($iFile.FullName)', '$(Read-FakeRecycleBytes $iFile.FullName)')
. ([scriptblock]::Create($text))
function Get-FakeRecycleDrives { [pscustomobject]@{ DriveType = if ($script:mode -eq 'removable-root-io') { 'Removable' } else { 'Fixed' }; IsReady = $true; Name = 'C:\'; RootDirectory = [pscustomobject]@{ FullName = 'C:\' } } }
function Get-Item {
    param($LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:mode -eq 'absent') { throw [Management.Automation.ItemNotFoundException]::new('Synthetic absent root') }
    if ($script:mode -eq 'denied') { throw [UnauthorizedAccessException]::new('Synthetic denied root') }
    if ($script:mode -in @('removable-root-io', 'fixed-root-io')) { throw [IO.IOException]::new('Synthetic device not ready') }
    [pscustomobject]@{ PSIsContainer = $true; Attributes = [IO.FileAttributes]::Directory; Name = $script:ownSid; FullName = 'C:\fixture\user' }
}
function Get-ChildItem {
    param($LiteralPath, $Filter, [switch]$Directory, [switch]$File, [switch]$Force, $ErrorAction)
    if ($Directory -and $script:mode -eq 'root-listing-denied') { throw [UnauthorizedAccessException]::new('Synthetic denied root listing') }
    if ($Directory -and $script:mode -eq 'other-reparse') { return [pscustomobject]@{ Name = 'other-sid'; FullName = 'C:\fixture\other'; Attributes = [IO.FileAttributes]::Directory -bor [IO.FileAttributes]::ReparsePoint } }
    if ($Directory -and $script:mode -eq 'own-reparse') { return [pscustomobject]@{ Name = $script:ownSid; FullName = 'C:\fixture\user'; Attributes = [IO.FileAttributes]::Directory -bor [IO.FileAttributes]::ReparsePoint } }
    if ($Directory) { return [pscustomobject]@{ Name = if ($script:mode -eq 'own-denied') { $script:ownSid } else { 'invalid-sid' }; FullName = 'C:\fixture\user'; Attributes = [IO.FileAttributes]::Directory } }
    if ($script:mode -eq 'sid-vanished') { throw [Management.Automation.ItemNotFoundException]::new('Synthetic vanished SID folder') }
    if ($script:mode -in @('enumeration-denied', 'own-denied')) { throw [UnauthorizedAccessException]::new('Synthetic denied metadata inventory') }
    $good = [pscustomobject]@{ FullName = 'C:\fixture\user\$Igood'; Length = 32; Attributes = [IO.FileAttributes]::Normal }
    if ($script:mode -eq 'mixed') { return @($good, [pscustomobject]@{ FullName = 'C:\fixture\user\$Ibad'; Length = 0; Attributes = [IO.FileAttributes]::Normal }) }
    if ($script:mode -eq 'oversize') { return [pscustomobject]@{ FullName = 'C:\fixture\user\$Ibig'; Length = 2MB; Attributes = [IO.FileAttributes]::Normal } }
    if ($script:mode -eq 'reparse-record') { return [pscustomobject]@{ FullName = 'C:\fixture\user\$Ilink'; Length = 32; Attributes = [IO.FileAttributes]::ReparsePoint } }
    return $good
}
function Read-FakeRecycleBytes {
    param($Path)
    if ($script:mode -eq 'vanished') { throw [IO.FileNotFoundException]::new('Synthetic purge') }
    if ($script:mode -eq 'vanished-dir') { throw [IO.DirectoryNotFoundException]::new('Synthetic purge') }
    if ($script:mode -eq 'sharing') { throw [IO.IOException]::new('Synthetic sharing violation') }
    if ($script:mode -eq 'record-denied') { throw [UnauthorizedAccessException]::new('Synthetic denied record') }
    if ($script:mode -in @('short', 'zero') -or ($script:mode -eq 'mixed' -and $Path -like '*$Ibad')) { return ,([byte[]]@(1, 2)) }
    $bytes = [byte[]]::new(32)
    [BitConverter]::GetBytes([long]$(if ($script:mode -eq 'unknown-version') { 9 } else { 2 })).CopyTo($bytes, 0)
    [BitConverter]::GetBytes([long]$(if ($script:mode -eq 'negative-size') { -1 } else { 1024 })).CopyTo($bytes, 8)
    [BitConverter]::GetBytes([int]2).CopyTo($bytes, 24)
    [Text.Encoding]::Unicode.GetBytes('X').CopyTo($bytes, 28)
    return ,$bytes
}
$script:ownSid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$script:mode = 'valid'
$result = Get-RecycleBinInfo
if ($result.partial -or $result.total -ne 1 -or $result.items[0].originalPath -ne 'X') { throw 'Readable metadata was not reported accurately.' }
$script:mode = 'enumeration-denied'; $result = Get-RecycleBinInfo
if ($result.partial -or $result.status -ne 'complete' -or $result.inaccessibleLocations -ne 0 -or $result.otherAccountLocations -ne 1 -or $result.coverage -ne 'limited' -or $result.coverageNote -notmatch 'administrator') { throw 'Another account''s unreadable Recycle Bin must be a scope limit, not an incomplete scan.' }
$script:mode = 'own-denied'; $result = Get-RecycleBinInfo
if (-not $result.partial -or $result.status -ne 'partial' -or $result.inaccessibleLocations -ne 1) { throw 'The current account''s unreadable Recycle Bin must stay incomplete.' }
$script:mode = 'root-listing-denied'; $result = Get-RecycleBinInfo
if ($result.partial -or $result.total -ne 1 -or $result.coverage -ne 'limited') { throw 'A standard account must still read its own bin when the shared root cannot be listed.' }
$script:mode = 'denied'; $result = Get-RecycleBinInfo
if (-not $result.partial -or $result.status -ne 'partial' -or $result.unreadableRecords -ne 0 -or $result.inaccessibleLocations -ne 1) { throw 'An inaccessible location was not counted accurately.' }
if ($result.message -ne 'Read 0 records; 1 location could not be read.') { throw "Partial message is not specific: $($result.message)" }
foreach ($mode in @('vanished', 'vanished-dir', 'sid-vanished')) {
    $script:mode = $mode; $result = Get-RecycleBinInfo
    if ($result.partial -or $result.status -ne 'complete' -or $result.unreadableRecords -ne 0 -or $result.total -ne 0 -or $result.coverage -or $result.coverageNote) { throw "A record that vanished during the scan must be skipped silently: $mode" }
}
foreach ($mode in @('short', 'zero', 'unknown-version', 'negative-size', 'sharing', 'oversize', 'reparse-record')) {
    $script:mode = $mode; $result = Get-RecycleBinInfo
    if ($result.partial -or $result.status -ne 'complete' -or $result.unreadableRecords -ne 1 -or $result.inaccessibleLocations -ne 0 -or $result.total -ne 0) { throw "Unreadable metadata must be counted without failing the scan: $mode" }
    if ($result.coverage -ne 'limited' -or $result.coverageNote -ne '1 Recycle Bin record has unreadable metadata and was skipped.') { throw "Unreadable metadata needs a coverage note: $mode / $($result.coverageNote)" }
}
$script:mode = 'mixed'; $result = Get-RecycleBinInfo
if ($result.partial -or $result.total -ne 1 -or $result.unreadableRecords -ne 1 -or $result.coverageNote -notmatch 'unreadable metadata') { throw 'A bad record next to a good one must not hide the good one or fail the scan.' }
$script:mode = 'record-denied'; $result = Get-RecycleBinInfo
if (-not $result.partial -or $result.unreadableRecords -ne 1 -or $result.message -ne 'Read 0 records; 1 record could not be read.') { throw 'A denied record in the current account''s own bin must stay incomplete.' }
$script:mode = 'own-reparse'; $result = Get-RecycleBinInfo
if (-not $result.partial -or $result.inaccessibleLocations -ne 1) { throw 'A reparse-point bin for the current account must stay incomplete.' }
$script:mode = 'other-reparse'; $result = Get-RecycleBinInfo
if ($result.partial -or $result.skippedLocations -ne 1 -or $result.coverageNote -notmatch 'could not be followed') { throw 'Another account''s reparse-point bin must be a note, not an incomplete scan.' }
$script:mode = 'removable-root-io'; $result = Get-RecycleBinInfo
if ($result.partial -or $result.total -ne 0) { throw 'Removable media that disappears mid-scan must not fail the scan.' }
$script:mode = 'fixed-root-io'; $result = Get-RecycleBinInfo
if (-not $result.partial -or $result.inaccessibleLocations -ne 1) { throw 'A fixed volume root that cannot be listed must stay incomplete.' }
$script:mode = 'absent'; $result = Get-RecycleBinInfo
if ($result.partial -or $result.total -ne 0) { throw 'An absent Recycle Bin was not reported as empty.' }
Write-Output 'Passed: readable, missing, denied, vanished and malformed Recycle Bin inventory.'
