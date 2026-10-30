$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
foreach ($name in @('Get-CleanupSummaryAllUsers', 'New-CleanupSummaryCategoryRecord')) {
    $definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $definition) { throw "Missing $name." }
    . ([scriptblock]::Create($definition.Extent.Text))
}

# Mock every provider; no real account or file is read.
$fixtureUser = 'wc-recent-fixture'
function Get-CimInstance { param($ClassName, $ErrorAction) [pscustomobject]@{ Special=$false; SID='S-1-5-21-999-999-999-999'; LocalPath="$env:SystemDrive\Users\$fixtureUser" } }
function Test-WCVisibleUserProfile { param($Name, $DisplayName, $Path) return $true }
function Test-IsAdmin { return $true }
function Get-LoggedInUsers { return @{ users=@($fixtureUser) } }
function Get-Item { param([string]$LiteralPath, [switch]$Force, $ErrorAction) if ($LiteralPath -like '*wpndatabase.db*') { return [pscustomobject]@{ Name=[IO.Path]::GetFileName($LiteralPath); FullName=$LiteralPath; PSIsContainer=$false; Length=2048; LastWriteTime=[datetime]'2026-01-02' } }; [pscustomobject]@{ FullName=$LiteralPath; PSIsContainer=$true } }
function Get-ChildItem {
    param([Alias('Path')]$LiteralPath, [string[]]$Include, [switch]$File, [switch]$Recurse, [int]$Depth, [string]$Filter, [switch]$Force, [Alias('EA')]$ErrorAction)
    if (-not $File) { throw 'Expected file inventory.' }
    if ($Include) {
        if (-not $Recurse) { throw 'Expected recursive database inventory.' }
        $root = @($LiteralPath)[0]
        $item = if ($Include -contains '*.wal') { 'fixture.db-wal' } else { 'fixture.db' }
        return [pscustomobject]@{ Name=$item; FullName="$root\nested\$item"; LastWriteTime=[datetime]'2026-01-02'; Length=2048 }
    }
    $item = if ($LiteralPath -like '*AutomaticDestinations') { 'auto.automaticDestinations-ms' } elseif ($LiteralPath -like '*CustomDestinations') { 'custom.customDestinations-ms' } else { 'example.lnk' }
    [pscustomobject]@{ Name=$item; BaseName=[IO.Path]::GetFileNameWithoutExtension($item); Extension=[IO.Path]::GetExtension($item); FullName="$LiteralPath\$item"; LastWriteTime=[datetime]'2026-01-02'; Length=2048 }
}
function New-Object {
    param([string]$ComObject)
    if ($ComObject -ne 'WScript.Shell') { throw 'Unexpected object.' }
    $shell = [pscustomobject]@{}
    $shell | Add-Member -MemberType ScriptMethod -Name CreateShortcut -Value { param($path) return [pscustomobject]@{ TargetPath='C:\Fixture\target.txt' } }
    return $shell
}

$recent = Get-CleanupSummaryAllUsers -CategoryIds 'recentFiles' -TargetUsers $fixtureUser
$row = @(@($recent.users)[0].categories[0].records)[0]
if ($row.name -ne 'example' -or $row.target -ne 'C:\Fixture\target.txt' -or $row.extension -ne '.lnk' -or $row.sizeBytes -ne 2048 -or -not $row.lastModified -or $row.path -notlike '*\example.lnk') { throw 'Other-account recent-file detail lost a field.' }
$jump = Get-CleanupSummaryAllUsers -CategoryIds 'jumpLists' -TargetUsers $fixtureUser
$rows = @(@($jump.users)[0].categories[0].records)
if ($rows.Count -ne 2 -or @($rows | Where-Object { $_.type -notin @('Automatic','Custom') -or $_.sizeKB -ne 2 -or -not $_.lastModified -or -not $_.path }).Count) { throw 'Other-account jump-list details lost a field.' }
$wal = Get-CleanupSummaryAllUsers -CategoryIds 'walFiles' -TargetUsers $fixtureUser
$row = @(@($wal.users)[0].categories[0].records)[0]
if ($row.name -ne 'fixture.db-wal' -or $row.path -notlike '*\nested\fixture.db-wal' -or $row.sizeKB -ne 2 -or -not $row.modified) { throw 'Other-account WAL details lost their path, size or date.' }
$recall = Get-CleanupSummaryAllUsers -CategoryIds 'recallDb' -TargetUsers $fixtureUser
$rows = @(@($recall.users)[0].categories[0].records)
if ($rows.Count -ne 4 -or @($rows | Where-Object { $_.path -notlike '*\nested\fixture.db' -or $_.sizeKB -ne 2 -or -not $_.modified }).Count -or @($rows | Where-Object { $_.source -eq 'ActionCenter' -and $_.path -like '*\Microsoft\Windows\ActionCenter\*' }).Count -ne 1 -or @($rows | Where-Object { $_.source -eq 'Recall' -and $_.path -like '*\Microsoft\Windows\Recall\*' }).Count -ne 1) { throw 'Other-account Recall inventory or details differ from the current-user scanner.' }
$crashes = Get-CleanupSummaryAllUsers -CategoryIds 'crashDumps' -TargetUsers $fixtureUser
$rows = @(@($crashes.users)[0].categories[0].records)
if ($rows.Count -ne 4 -or @($rows | Where-Object { -not $_.source -or $_.path -notlike '*\example.lnk' -or $_.sizeKB -ne 2 -or -not $_.modified }).Count) { throw 'Other-account crash dump records lost their source, path, size or date.' }
foreach ($category in @('webCache','thumbnailDb','notificationDb','activitiesTimeline','rdpBitmapCache')) {
    $summary = Get-CleanupSummaryAllUsers -CategoryIds $category -TargetUsers $fixtureUser
    $rows = @(@($summary.users)[0].categories[0].records)
    if (-not $rows.Count -or @($rows | Where-Object { -not $_.name -or -not $_.path -or $_.sizeKB -ne 2 -or -not $_.modified }).Count) { throw "Other-account $category records lost file details." }
}
Write-Output 'Passed: other-account cleanup records retain their returned file details.'
