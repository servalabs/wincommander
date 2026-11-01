$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $path), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup source did not parse.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-CleanupSummaryAllUsers' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'New-CleanupSummaryCategoryRecord' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
foreach ($name in @('Test-CleanupSummaryDetailSize', 'ConvertTo-CleanupCanonicalJson', 'Get-CleanupSummaryPage')) {
    $definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    . ([scriptblock]::Create($definition.Extent.Text))
}
$script:mode='other'; $script:delegated='success'
function Test-IsAdmin { $true }
function Test-WCVisibleUserProfile { param($Name,$DisplayName,$Path) $true }
function Get-LoggedInUsers { @{ users=@() } }
function Get-CimInstance {
    [CmdletBinding()] param([string]$ClassName)
    if ($script:mode -eq 'inventoryDenied') { throw [UnauthorizedAccessException]::new('Denied inventory') }
    $sid=if ($script:mode -eq 'current') { ([Security.Principal.WindowsIdentity]::GetCurrent()).User.Value } else { 'S-1-5-21-111-222-333-444' }
    [pscustomobject]@{ SID=$sid; Special=$false; LocalPath="$env:SystemDrive\Users\CleanupMock" }
}
function Get-Item {
    [CmdletBinding()] param([string]$LiteralPath,[switch]$Force)
    if ($LiteralPath -like 'Registry::*' -or $LiteralPath -like '*NTUSER.DAT') { throw [System.Management.Automation.ItemNotFoundException]::new('Missing hive') }
    if ($script:mode -eq 'deniedFiles') { throw [UnauthorizedAccessException]::new('Denied profile') }
    if ($script:mode -eq 'missingFiles') { throw [System.Management.Automation.ItemNotFoundException]::new('Missing folder') }
    [pscustomobject]@{ Name='Mock'; PSIsContainer=$true }
}
function Get-ChildItem {
    [CmdletBinding()] param([Parameter(Position=0)][Alias('LiteralPath')][string]$Path,[switch]$File,[switch]$Force)
    if ($script:mode -eq 'deniedEnumeration') { throw [UnauthorizedAccessException]::new('Denied enumeration') }
    [pscustomobject]@{ Name='Mock1.lnk'; BaseName='Mock1'; Extension='.lnk'; FullName='C:\Mock1.lnk'; Length=1024; LastWriteTime=[datetime]'2026-01-01' }
    [pscustomobject]@{ Name='Mock2.lnk'; BaseName='Mock2'; Extension='.lnk'; FullName='C:\Mock2.lnk'; Length=1024; LastWriteTime=[datetime]'2026-01-01' }
}
function Get-RecentFiles {
    if ($script:delegated -eq 'error') { return @{ error=$true; message='Mock reader failed' } }
    if ($script:delegated -eq 'partial') { return @{ partial=$true; total=7; coverage='Selected locations only'; sourcePath='C:\Fixture'; message='Mock reader incomplete'; entries=@(@{ name='Mock' }) } }
    if ($script:delegated -eq 'lateError') {
        return @{ error=$true; message='Reader failed after collecting an observation'; total=1; entries=@(@{ name='Observed'; path='C:\Fixture\Observed' }) }
    }
    if ($script:delegated -eq 'lateMany') {
        return @{ error=$true; message='Reader failed after collecting observations'; total=5001; entries=@(0..5000 | ForEach-Object { @{ name="Observed $_" } }) }
    }
    @{ total=2; entries=@(@{ name='Mock1' },@{ name='Mock2' }) }
}
$mixed=Get-CleanupSummaryAllUsers -CategoryIds 'shellBags,recentFiles' -TargetUsers 'CleanupMock'
$user=$mixed.users[0]
$registry=@($user.categories | Where-Object { $_.id -eq 'shellBags' })[0]
$files=@($user.categories | Where-Object { $_.id -eq 'recentFiles' })[0]
if ($user.hiveAvailable -or -not $user.hiveError -or $registry.count -ne -1 -or -not $registry.error -or $registry.status -ne 'error') { throw 'An unavailable hive was reported as zero.' }
if ($files.count -ne 2 -or $files.error -or $files.status -ne 'complete' -or $user.total -ne -1 -or $user.knownTotal -ne 2 -or $user.complete) { throw 'Mixed known and unknown user totals were misrepresented.' }
foreach ($mode in @('deniedFiles','deniedEnumeration')) {
    $script:mode=$mode
    $result=Get-CleanupSummaryAllUsers -CategoryIds 'recentFiles' -TargetUsers 'CleanupMock'
    if ($result.users[0].categories[0].count -ne -1 -or -not $result.users[0].categories[0].error) { throw "$mode was reported as empty." }
}
$script:mode='missingFiles'
$empty=Get-CleanupSummaryAllUsers -CategoryIds 'recentFiles' -TargetUsers 'CleanupMock'
if ($empty.users[0].categories[0].count -ne 0 -or $empty.users[0].categories[0].error -or $empty.users[0].total -ne 0) { throw 'Absent trace folder was not an explicit zero.' }
$sidResult=Get-CleanupSummaryAllUsers -CategoryIds 'recentFiles' -TargetUsers 'S-1-5-21-111-222-333-444'
if ($sidResult.error -or $sidResult.users[0].sid -ne 'S-1-5-21-111-222-333-444') { throw 'SID selection did not return the selected profile.' }
function Test-IsAdmin { $false }
$script:mode='current'
$unauthorized=Get-CleanupSummaryAllUsers -CategoryIds 'recentFiles' -TargetUsers 'S-1-5-21-111-222-333-444'
if (-not $unauthorized.error -or @($unauthorized.users).Count -ne 0) { throw 'Unauthorized target was replaced with the current account.' }
function Test-IsAdmin { $true }
$script:delegated='error'
$result=Get-CleanupSummaryAllUsers -CategoryIds 'recentFiles'
if ($result.users[0].categories[0].count -ne -1 -or -not $result.users[0].categories[0].error) { throw 'Delegated error was reported as zero or complete.' }
$script:delegated='partial'
$result=Get-CleanupSummaryAllUsers -CategoryIds 'recentFiles'
$category=$result.users[0].categories[0]
if (-not $category.partial -or $category.status -ne 'partial' -or @($category.records).Count -ne 1 -or $result.users[0].complete -or $result.users[0].total -ne -1 -or $category.coverage -ne 'Selected locations only' -or $category.scanFacts.sourcePath -ne 'C:\Fixture' -or $category.message -ne 'Mock reader incomplete') { throw 'Delegated partial scan lost records or its explanation, or was reported complete.' }
$script:delegated='lateError'
$result=Get-CleanupSummaryAllUsers -CategoryIds 'recentFiles'
$category=$result.users[0].categories[0]
if (-not $category.error -or $category.count -ne -1 -or @($category.records).Count -ne 1 -or $category.records[0].path -ne 'C:\Fixture\Observed' -or $result.users[0].complete) { throw "Late failure: error=$($category.error), count=$($category.count), rows=$(@($category.records).Count), complete=$($result.users[0].complete)" }
$script:delegated='lateMany'
$preview = (Get-CleanupSummaryAllUsers -CategoryIds 'recentFiles' -TargetUsers 'CleanupMock').users[0].categories[0]
if (-not $preview.error -or $preview.count -ne -1 -or @($preview.records).Count -ne 500 -or -not $preview.detailRefreshAvailable) { throw 'Failed preview did not retain observations and offer refresh with an unknown total.' }
$first = (Get-CleanupSummaryAllUsers -CategoryIds 'recentFiles' -TargetUsers 'CleanupMock' -FullDetail $true).users[0].categories[0]
if (-not $first.error -or -not $first.sourceFailed -or $first.count -ne -1 -or @($first.records).Count -ne 5000 -or $first.nextOffset -ne 5000 -or $first.totalRows -ne 5001) { throw 'Failed scan did not page observations without claiming success.' }
$last = (Get-CleanupSummaryAllUsers -CategoryIds 'recentFiles' -TargetUsers 'CleanupMock' -FullDetail $true -DetailOffset 5000 -SnapshotHash $first.snapshotHash).users[0].categories[0]
if (-not $last.error -or -not $last.sourceFailed -or $last.count -ne -1 -or @($last.records).Count -ne 1 -or $null -ne $last.nextOffset) { throw 'Failed scan continuation did not retain unknown total.' }
$changed = (Get-CleanupSummaryAllUsers -CategoryIds 'recentFiles' -TargetUsers 'CleanupMock' -FullDetail $true -DetailOffset 5000 -SnapshotHash ('sha256:' + ('b' * 64))).users[0].categories[0]
if (-not $changed.error -or $changed.sourceFailed -or @($changed.records).Count -ne 0) { throw 'Changed snapshot returned a verified failed-source page.' }
$one = @(@{ name='Observed' })
$snapshot = Get-CleanupSummaryPage -Records $one -Count -1 -Category 'recentFiles' -ProfileSid 'S-1-5-21-111-222-333-444' -Offset 0 -ExpectedSnapshot '' -SourceFailure 'First failure'
foreach ($change in @(@{ sid='S-1-5-21-111-222-333-445'; category='recentFiles'; failure='First failure' }, @{ sid='S-1-5-21-111-222-333-444'; category='rdpHistory'; failure='First failure' }, @{ sid='S-1-5-21-111-222-333-444'; category='recentFiles'; failure='Changed failure' })) {
    $rejected = $false
    try { $null = Get-CleanupSummaryPage -Records $one -Count -1 -Category $change.category -ProfileSid $change.sid -Offset 1 -ExpectedSnapshot $snapshot.snapshotHash -SourceFailure $change.failure }
    catch { $rejected = $true }
    if (-not $rejected) { throw 'A changed failed snapshot was accepted.' }
}
$script:delegated='success'
$success=Get-CleanupSummaryAllUsers -CategoryIds 'recentFiles'
if ($success.users[0].total -ne 2 -or -not $success.users[0].complete) { throw 'A successful current-user result changed.' }
$script:mode='inventoryDenied'
$denied=Get-CleanupSummaryAllUsers -CategoryIds 'recentFiles'
if (-not $denied.error -or $denied.status -ne 'error') { throw 'Denied profile inventory was reported as no users.' }
Write-Output 'Passed: unavailable hive and delegated failures stay unknown; mixed totals; denied profile/enumeration; missing folder zero; current-user success; denied inventory.'
