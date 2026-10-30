param([switch]$HashOnly)
$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module could not be parsed.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-CleanupSummaryAllUsers' }, $true)
if (-not $definition) { throw 'Account summary command is missing.' }
. ([scriptblock]::Create($definition.Extent.Text))
foreach ($name in @('Test-CleanupSummaryDetailSize', 'ConvertTo-CleanupCanonicalJson', 'Get-CleanupSummaryPage', 'New-CleanupSummaryCategoryRecord')) {
    $helper = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $helper) { throw "Missing helper $name" }
    . ([scriptblock]::Create($helper.Extent.Text))
}
if ($HashOnly) {
    $record = @{}; $record['z'] = 'last'; $record['a'] = 'first'
    $page = Get-CleanupSummaryPage -Records @($record) -Count 1 -Category 'psHistory' -ProfileSid 'S-1-5-21-1' -Offset 0 -ExpectedSnapshot ''
    Write-Output $page.snapshotHash
    exit 0
}
$record = @{}; $record['a'] = 'first'; $record['z'] = 'last'
$localHash = (Get-CleanupSummaryPage -Records @($record) -Count 1 -Category 'psHistory' -ProfileSid 'S-1-5-21-1' -Offset 0 -ExpectedSnapshot '').snapshotHash
$childHash = & powershell -NoProfile -ExecutionPolicy Bypass -File $PSCommandPath -HashOnly
if ($LASTEXITCODE -ne 0 -or $localHash -cne $childHash) { throw 'Canonical snapshot hashes differ across processes or key ordering.' }
try { $null = Get-CleanupSummaryPage -Records @($record) -Count 1 -Category 'psHistory' -ProfileSid 'S-1-5-21-2' -Offset 1 -ExpectedSnapshot $localHash; throw 'A different account reused the snapshot.' }
catch { if ($_.Exception.Message -notmatch 'changed while loading') { throw } }
try { $null = Get-CleanupSummaryPage -Records @($record) -Count 1 -Category 'rdpHistory' -ProfileSid 'S-1-5-21-1' -Offset 1 -ExpectedSnapshot $localHash; throw 'A different category reused the snapshot.' }
catch { if ($_.Exception.Message -notmatch 'changed while loading') { throw } }
$originalSizeCheck = ${function:Test-CleanupSummaryDetailSize}
function Test-CleanupSummaryDetailSize { param($Records, $Items) return @($Records).Count -le 2 }
try {
    $adaptive = Get-CleanupSummaryPage -Records @(@{id=1}, @{id=2}, @{id=3}, @{id=4}, @{id=5}) -Count 5 -Category 'psHistory' -ProfileSid 'S-1-5-21-1' -Offset 0 -ExpectedSnapshot ''
    if ($adaptive.records.Count -ne 2 -or $adaptive.nextOffset -ne 2) { throw 'An oversized page was not reduced safely.' }
} finally { Set-Item -Path Function:\Test-CleanupSummaryDetailSize -Value $originalSizeCheck }
$multiUser = Get-CleanupSummaryAllUsers -CategoryIds 'psHistory' -TargetUsers 'first,second' -FullDetail $true
if (-not $multiUser.error -or $multiUser.userCount -ne 0) { throw 'Full details accepted multiple users.' }
$multiCategory = Get-CleanupSummaryAllUsers -CategoryIds 'psHistory,rdpHistory' -TargetUsers 'first' -FullDetail $true
if (-not $multiCategory.error -or $multiCategory.userCount -ne 0) { throw 'Full details accepted multiple categories.' }
function Get-CimInstance { param($ClassName, $ErrorAction) [pscustomobject]@{ Special=$false; SID='S-1-5-21-999-999-999-999'; LocalPath="$env:SystemDrive\Users\another-user" } }
function Test-WCVisibleUserProfile { param($Name, $DisplayName, $Path) return $true }
function Test-IsAdmin { return $false }
$unauthorized = Get-CleanupSummaryAllUsers -CategoryIds 'psHistory' -TargetUsers 'another-user' -FullDetail $true
if (-not $unauthorized.error -or $unauthorized.userCount -ne 0) { throw 'A non-admin caller could access another account.' }
$records = @(0..10049 | ForEach-Object { @{ id=$_; command="synthetic-$_" } })
$first = Get-CleanupSummaryPage -Records $records -Count 10050 -Category 'psHistory' -ProfileSid 'S-1-5-21-1' -Offset 0 -ExpectedSnapshot ''
if ($first.records.Count -ne 5000 -or $first.nextOffset -ne 5000 -or $first.totalRows -ne 10050) { throw 'First page did not preserve the complete snapshot count.' }
$later = Get-CleanupSummaryPage -Records $records -Count 10050 -Category 'psHistory' -ProfileSid 'S-1-5-21-1' -Offset 10000 -ExpectedSnapshot $first.snapshotHash
if ($later.records.Count -ne 50 -or $null -ne $later.nextOffset -or $later.records[0].id -ne 10000) { throw 'The page beyond 10,000 rows is incorrect.' }
$summary = New-CleanupSummaryCategoryRecord -Category 'psHistory' -Count 10050 -items @() -records $records -ScanError $null -previewMax 50000
if ($summary.truncated) { throw 'A complete snapshot was incorrectly marked truncated.' }
if (-not ($summary.truncated -or $null -ne $first.nextOffset) -or ($summary.truncated -or $null -ne $later.nextOffset)) { throw 'Page completeness did not distinguish the first and final page.' }
$limited = New-CleanupSummaryCategoryRecord -Category 'psHistory' -Count 10051 -items @() -records $records -ScanError $null -previewMax 50000
if (-not $limited.truncated) { throw 'A capped source snapshot must stay truncated after the final page.' }
$records[0].command = 'changed'
try { $null = Get-CleanupSummaryPage -Records $records -Count 10050 -Category 'psHistory' -ProfileSid 'S-1-5-21-1' -Offset 1000 -ExpectedSnapshot $first.snapshotHash; throw 'Changed snapshot was accepted.' }
catch { if ($_.Exception.Message -notmatch 'changed while loading') { throw } }
$largeRecords = @(0..50000)
$largeFirst = Get-CleanupSummaryPage -Records $largeRecords -Count 50001 -Category 'psHistory' -ProfileSid 'S-1-5-21-1' -Offset 0 -ExpectedSnapshot ''
$largeLast = Get-CleanupSummaryPage -Records $largeRecords -Count 50001 -Category 'psHistory' -ProfileSid 'S-1-5-21-1' -Offset 50000 -ExpectedSnapshot $largeFirst.snapshotHash
if ($largeLast.records.Count -ne 1 -or $largeLast.records[0] -ne 50000 -or $null -ne $largeLast.nextOffset) { throw 'Records beyond the old 50,000-row limit were not returned.' }
$largeText = 'x' * 900000
$largeSnapshot = @(1..40 | ForEach-Object { @{ id=$_; value=$largeText } })
$largePage = Get-CleanupSummaryPage -Records $largeSnapshot -Count 40 -Category 'psHistory' -ProfileSid 'S-1-5-21-1' -Offset 0 -ExpectedSnapshot ''
if ($largePage.records.Count -lt 1 -or $largePage.records.Count -ge 40 -or $null -eq $largePage.nextOffset) { throw 'A snapshot larger than 32 MiB did not yield a bounded page.' }
Write-Output 'Passed: full details reject multi-user and multi-category requests before reading profiles.'
