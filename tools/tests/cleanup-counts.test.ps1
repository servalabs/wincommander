$ErrorActionPreference = 'Stop'

# Exercise the real function bodies with synthetic registry responses only.
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null
$errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup.ps1' }
foreach ($name in @('Get-AmcacheEntries', 'Get-NTUserTraces', 'Get-SQLiteWALList', 'Get-RecallDatabaseInfo', 'Convert-CleanupClipboardHistoryResult', 'Get-ClipboardHistoryStatus', 'Convert-CleanupSummaryHistory', 'Test-CleanupSummaryDetailSize', 'New-CleanupSummaryCategoryRecord')) {
    $function = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $function) { throw "Missing function $name" }
    . ([scriptblock]::Create($function.Extent.Text))
}

function Test-Path {
    param($Path, $ErrorAction)
    if ($script:coverageFixture) { return $true }
    if (-not $script:populated) { return $false }
    return $Path -match '(InventoryApplication|InventoryApplicationFile|RunMRU|TypedPaths)$'
}
function Get-Item {
    param($Path, $LiteralPath, $ErrorAction)
    if ($script:coverageFixture) {
        if ($script:deniedRoot) { throw [System.UnauthorizedAccessException]::new('Synthetic access denied') }
        return [pscustomobject]@{ Name = 'fixture-root' }
    }
    if ($LiteralPath -or -not (Test-Path -Path $Path)) {
        throw [System.Management.Automation.ItemNotFoundException]::new('Synthetic item is absent')
    }
    return [pscustomobject]@{ Name = 'fixture-category' }
}
function Get-ChildItem {
    param($Path, $ErrorAction, $Include, [switch]$Recurse, [int]$Depth, [switch]$Force)
    if ($script:coverageFixture) {
        if ($script:deniedContents) { throw [System.UnauthorizedAccessException]::new('Synthetic access denied') }
        for ($index = 0; $index -lt $script:fixtureCount; $index++) {
            [pscustomobject]@{ Name = "fixture-$index.db"; FullName = "C:\fixture\root\fixture-$index.db"; Length = 1024; LastWriteTime = [datetime]'2026-01-01'; PSIsContainer = $false }
        }
        return
    }
    $count = if ($Path -match 'InventoryApplicationFile$') { 1 } else { 2 }
    foreach ($index in 1..$count) { [pscustomobject]@{ PSChildName = "fixture-$index"; PSPath = "fixture-$index" } }
}
function Get-ItemProperty {
    param($Path, $Name, [Alias('EA')]$ErrorAction)
    if ($script:clipboardFixture) {
        if ($Path -match '^HKCU:') { return [pscustomobject]@{ EnableClipboardHistory = $script:historyEnabled; EnableCloudClipboard = 1 } }
        if ($Name -eq 'AllowClipboardHistory') { return [pscustomobject]@{ AllowClipboardHistory = 1 } }
        return [pscustomobject]@{ AllowCrossDeviceClipboard = 1 }
    }
    if ($Path -match 'RunMRU$') { return [pscustomobject]@{ MRUList = 'ab'; a = 'fixture-a'; b = 'fixture-b' } }
    if ($Path -match 'TypedPaths$') { return [pscustomobject]@{ url1 = 'fixture-path' } }
    return [pscustomobject]@{ Name = ''; ProductName = 'Synthetic App'; BinFileVersion = ''; LowerCaseLongPath = 'C:\fixture\app.exe'; LongPathHash = '' }
}
function Get-CleanupClipboardHistory { param([bool]$IncludePreviews = $false) $script:clipboardReads++; $script:clipboardPreviewRequested = $IncludePreviews; return $script:clipboardResult }
function Assert-Count($actual, $expected, $label) {
    if ($actual -ne $expected) { throw "$label`: expected $expected, got $actual" }
}

$summaryFunction = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-CleanupSummaryAllUsers' }, $true)
$summaryRecordCalls = @($summaryFunction.Body.FindAll({ param($node) $node -is [System.Management.Automation.Language.CommandAst] -and $node.GetCommandName() -eq 'New-CleanupSummaryCategoryRecord' }, $true))
Assert-Count $summaryRecordCalls.Count 1 'Delegated and nondelegated scans share one category-record projection'
foreach ($category in @('walFiles', 'recallDb')) {
    $record = New-CleanupSummaryCategoryRecord -Category $category -Count 0 -items @() -records @() -ScanError $null -previewMax 500
    Assert-Count $record.coverage 'limited' "$category user coverage"
    Assert-Count $record.status 'scoped' "$category user status"
    Assert-Count $record.count 0 "$category scoped empty count"
    $record = New-CleanupSummaryCategoryRecord -Category $category -Count -1 -items @() -records @() -ScanError 'Synthetic scan error' -previewMax 500
    Assert-Count $record.status 'error' "$category user error status"
    Assert-Count $record.error $true "$category user error flag"
}
$record = New-CleanupSummaryCategoryRecord -Category 'recentFiles' -Count 0 -items @() -records @() -ScanError $null -previewMax 500
Assert-Count $record.status 'complete' 'Unrestricted category status remains complete'
if ($record.ContainsKey('coverage')) { throw 'Unexpected coverage flag for unrestricted category' }
$longCommand = 'example-command ' + ('x' * 120)
$history = Convert-CleanupSummaryHistory -Lines @('first', $longCommand, 'last') -PreviewMax 2
Assert-Count $history.count 3 'Other-user command count'
Assert-Count $history.records[1].command $longCommand 'Other-user command remains complete'
$record = New-CleanupSummaryCategoryRecord -Category 'psHistory' -Count $history.count -items $history.items -records $history.records -ScanError $null -previewMax 2
Assert-Count $record.truncated $true 'Bounded other-user preview reports truncation'
$group = New-CleanupSummaryCategoryRecord -Category 'ntUserTraces' -Count 2 -items @() -records @(@{ name='RunMRU'; count=2; entries=@(@{value='one'}, @{value='two'}) }) -ScanError $null -previewMax 500
Assert-Count $group.truncated $false 'Complete grouped activity does not report truncation'
Assert-Count (Test-CleanupSummaryDetailSize -Records @(@{ command = 'small' }) -Items @('small')) $true 'Small detail response passes size bound'
Assert-Count (Test-CleanupSummaryDetailSize -Records @(@{ command = ('x' * 8388608) }) -Items @()) $false 'Oversized detail response is rejected'

$oldSystemRoot = $env:SystemRoot
$oldLocalAppData = $env:LOCALAPPDATA
$oldAppData = $env:APPDATA
try {
    # The synthetic path does not exist; no Windows hive or registry is read.
    $env:SystemRoot = Join-Path $env:TEMP ('wincommander-count-fixture-' + [guid]::NewGuid())
    $script:populated = $false
    $amcache = Get-AmcacheEntries
    $user = Get-NTUserTraces
    Assert-Count $amcache.total 0 'Empty Amcache total'
    Assert-Count $user.total 0 'Empty user activity total'

    $script:populated = $true
    $amcache = Get-AmcacheEntries
    $user = Get-NTUserTraces
    Assert-Count $amcache.total 3 'Populated Amcache total'
    Assert-Count $user.total 3 'Populated user activity total'
    Assert-Count $amcache.entries[0]['count'] 2 'Application category'
    Assert-Count $amcache.entries[1]['count'] 1 'Application files category'
    Assert-Count $amcache.entries[0]['sample'][0]['name'] 'Synthetic App' 'Application sample name'
    Assert-Count $amcache.entries[0]['sample'][0]['path'] 'C:\fixture\app.exe' 'Application sample path'
    Assert-Count $user.sections[0]['count'] 2 'RunMRU section'
    Assert-Count $user.sections[1]['count'] 1 'TypedPaths section'

    $script:clipboardFixture = $true
    $script:clipboardReads = 0
    $script:historyEnabled = 1
    $script:clipboardResult = Convert-CleanupClipboardHistoryResult -Result ([pscustomobject]@{ Status = 'Success'; Items = @() })
    $clipboard = Get-ClipboardHistoryStatus
    Assert-Count $script:clipboardPreviewRequested $false 'Default clipboard inventory excludes content'
    Assert-Count $clipboard.historyReadStatus 'complete' 'Empty clipboard history coverage'
    Assert-Count $clipboard.historyItems.Count 0 'Enabled empty clipboard history'
    $script:clipboardResult = Convert-CleanupClipboardHistoryResult -Result ([pscustomobject]@{ Status = 'Success'; Items = @('secret-fixture', 'private-fixture') })
    $clipboard = Get-ClipboardHistoryStatus
    Assert-Count $clipboard.historyItems.Count 2 'Populated clipboard history'
    if (($clipboard | ConvertTo-Json -Depth 5) -match 'secret-fixture|private-fixture') { throw 'Clipboard content leaked into the response' }
    $preview = Convert-CleanupClipboardHistoryResult -Result ([pscustomobject]@{ Status = 'Success'; Items = @('first', 'unreadable') }) -ReadPreview {
        param($item)
        if ($item -eq 'unreadable') { throw 'Synthetic read failure' }
        @{ type = 'text'; preview = 'Sample text'; charCount = 11 }
    }
    Assert-Count $preview.items.Count 2 'Preview retains unreadable items'
    Assert-Count $preview.items[0].preview 'Sample text' 'Explicit preview returns readable content'
    Assert-Count $preview.items[1].preview '[Content unavailable]' 'Unreadable preview is identified'
    $null = Get-ClipboardHistoryStatus -IncludePreviews $true
    Assert-Count $script:clipboardPreviewRequested $true 'Explicit local request enables preview reading'
    $script:clipboardResult = Convert-CleanupClipboardHistoryResult -Result ([pscustomobject]@{ Status = 'AccessDenied'; Items = @() })
    $clipboard = Get-ClipboardHistoryStatus
    Assert-Count $clipboard.historyReadStatus 'unavailable' 'Denied clipboard history coverage'
    Assert-Count $clipboard.partial $true 'Denied clipboard history is partial'
    $script:historyEnabled = 0
    $script:clipboardResult = Convert-CleanupClipboardHistoryResult -Result ([pscustomobject]@{ Status = 'ClipboardHistoryDisabled'; Items = @() })
    $readsBefore = $script:clipboardReads
    $clipboard = Get-ClipboardHistoryStatus
    Assert-Count $script:clipboardReads ($readsBefore + 1) 'Disabled history still attempts an inventory'
    Assert-Count $clipboard.historyReadStatus 'disabled' 'Disabled clipboard history coverage'
    Assert-Count $clipboard.partial $true 'Unreadable disabled history is partial'
    $script:clipboardResult = Convert-CleanupClipboardHistoryResult -Result ([pscustomobject]@{ Status = 'Success'; Items = @() })
    $clipboard = Get-ClipboardHistoryStatus
    Assert-Count $clipboard.historyReadStatus 'complete' 'Successful inventory proves disabled history is empty'
    Assert-Count $clipboard.historyItems.Count 0 'Successfully inventoried empty history'
    Assert-Count $clipboard.partial $false 'Successful inventory is complete'
    $script:clipboardFixture = $false

    $script:coverageFixture = $true
    $env:LOCALAPPDATA = 'C:\fixture\Local'
    $env:APPDATA = 'C:\fixture\Roaming'
    foreach ($fixtureCount in @(0, 99, 100, 101)) {
        $script:fixtureCount = $fixtureCount
        $wal = Get-SQLiteWALList
        $recall = Get-RecallDatabaseInfo
        $expectedPartial = $fixtureCount -ge 100
        Assert-Count $wal.partial $expectedPartial "WAL limit state for $fixtureCount files per root"
        Assert-Count $recall.partial $expectedPartial "Recall limit state for $fixtureCount files per source"
        Assert-Count $wal.truncated $expectedPartial 'WAL truncation flag'
        Assert-Count $recall.truncated $expectedPartial 'Recall truncation flag'
        Assert-Count $wal.coverage 'limited' 'WAL coverage classification'
        Assert-Count $recall.coverage 'limited' 'Recall coverage classification'
        if ($wal.coverageNote -notmatch 'depth 5') { throw 'WAL depth boundary was not reported' }
        Assert-Count $wal.total (2 * [math]::Min($fixtureCount, 100)) 'WAL listed count'
        Assert-Count $recall.total (4 * [math]::Min($fixtureCount, 100)) 'Recall listed count'
    }
    $script:deniedRoot = $true
    Assert-Count (Get-SQLiteWALList).error $true 'Denied WAL root is an error'
    Assert-Count (Get-RecallDatabaseInfo).error $true 'Denied Recall root is an error'
    $script:deniedRoot = $false
    $script:deniedContents = $true
    Assert-Count (Get-SQLiteWALList).error $true 'Denied WAL enumeration is an error'
    Assert-Count (Get-RecallDatabaseInfo).error $true 'Denied Recall enumeration is an error'
    Write-Output 'Cleanup count fixtures passed.'
} finally {
    $env:SystemRoot = $oldSystemRoot
    $env:LOCALAPPDATA = $oldLocalAppData
    $env:APPDATA = $oldAppData
}
