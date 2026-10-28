$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $path), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup source did not parse.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-CleanupSummaryAllUsers' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
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
    [CmdletBinding()] param([Parameter(Position=0)][string]$Path,[switch]$File,[switch]$Force)
    if ($script:mode -eq 'deniedEnumeration') { throw [UnauthorizedAccessException]::new('Denied enumeration') }
    [pscustomobject]@{ Name='Mock1.lnk'; BaseName='Mock1' }
    [pscustomobject]@{ Name='Mock2.lnk'; BaseName='Mock2' }
}
function Get-RecentFiles {
    if ($script:delegated -eq 'error') { return @{ error=$true; message='Mock reader failed' } }
    if ($script:delegated -eq 'partial') { return @{ partial=$true; total=7; message='Mock reader incomplete'; entries=@(@{ name='Mock' }) } }
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
$script:mode='current'
foreach ($state in @('error','partial')) {
    $script:delegated=$state
    $result=Get-CleanupSummaryAllUsers -CategoryIds 'recentFiles'
    if ($result.users[0].categories[0].count -ne -1 -or -not $result.users[0].categories[0].error) { throw "Delegated $state was reported as zero or complete." }
}
$script:delegated='success'
$success=Get-CleanupSummaryAllUsers -CategoryIds 'recentFiles'
if ($success.users[0].total -ne 2 -or -not $success.users[0].complete) { throw 'A successful current-user result changed.' }
$script:mode='inventoryDenied'
$denied=Get-CleanupSummaryAllUsers -CategoryIds 'recentFiles'
if (-not $denied.error -or $denied.status -ne 'error') { throw 'Denied profile inventory was reported as no users.' }
Write-Output 'Passed: unavailable hive and delegated failures stay unknown; mixed totals; denied profile/enumeration; missing folder zero; current-user success; denied inventory.'
