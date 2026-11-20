param([string]$CleanupPath)
# Fleet forensic "view" projections: contract regressions found by live testing.
# Collectors are replaced with synthetic fixtures; nothing is read from the
# live device except the read-only ShellBags shape check at the end.
$ErrorActionPreference = 'Stop'
if (-not $CleanupPath) { $CleanupPath = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1' }
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $CleanupPath).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
foreach ($name in @('New-FleetForensicProjection', 'Get-FleetForensicProjection', 'Get-ShellBags')) {
    $definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $definition) { throw "$name is missing." }
    . ([scriptblock]::Create($definition.Extent.Text))
}

# The column types Pro's parse_columns accepts (fleet_forensic_view.rs) and the
# Fleet server's ForensicDatasetColumnKind can represent. `boolean` is not one.
$supportedColumnTypes = @('text', 'opaque-id', 'redacted', 'number', 'timestamp')
function Assert-ProjectionContract($result, [string]$label) {
    if ($result.error) { throw "$label returned an error: $($result.message)" }
    if ($result.source -ne 'wincommander.system_cleanup' -or $result.projection_contract_version -ne 2) { throw "$label lost its projection envelope." }
    foreach ($column in @($result.columns)) {
        if ($column.type -notin $supportedColumnTypes) { throw "$label column '$($column.key)' has type '$($column.type)', which Pro rejects." }
    }
    $rows = @($result.records)
    if ($rows.Count -gt 200 -or $result.total -lt $rows.Count -or (-not $result.truncated -and $result.total -ne $rows.Count)) {
        throw "$label total/truncated disagree with its rows (rows=$($rows.Count) total=$($result.total) truncated=$($result.truncated))."
    }
}

$script:failures = [System.Collections.Generic.List[string]]::new()
function Test-Section([string]$Name, [scriptblock]$Body) { try { & $Body } catch { $script:failures.Add("$Name : $($_.Exception.Message)") } }

Test-Section 'booleans' {
# --- 1. Boolean source values are projected as supported text columns -------
function Get-ShadowCopies { @{ vssRunning = $true; copies = @(@{ id = '{abc}'; drive = 'C:'; created = '2026-11-20 10:00:00'; clientAccessible = $true; persistent = $false; stateStr = 'Created' }) } }
function Get-NTFSJournals { @{ journals = @(@{ drive = 'C:'; present = $true; journalId = '1234'; maxSize = '32 MB' }, @{ drive = 'D:'; present = $false; journalId = ''; maxSize = '' }) } }
$shadow = Get-FleetForensicProjection -Category shadow_copies -FullDetail $true
Assert-ProjectionContract $shadow 'shadow_copies'
if ($shadow.records[0].clientAccessible -ne 'Yes' -or $shadow.records[0].persistent -ne 'No') { throw 'Shadow-copy booleans must be Yes/No text.' }
$journals = Get-FleetForensicProjection -Category ntfs_journals -FullDetail $true
Assert-ProjectionContract $journals 'ntfs_journals'
if ($journals.records[0].present -ne 'Yes' -or $journals.records[1].present -ne 'No' -or $journals.total -ne 2) { throw 'NTFS-journal presence must be Yes/No text.' }

}
Test-Section 'empty-sources' {
# --- 2. An empty source is a valid zero-row table, not a binding failure ----
function Get-NotepadStateFiles { @{ files = @() } }
function Get-CrashDumpList { @{ dumps = @(); partial = $false } }
function Get-PrintSpoolerInfo { @{ files = @(); spoolerState = 'Running' } }
function Get-BranchCacheInfo { @{ files = @(); partial = $false } }
function Get-EventTranscriptInfo { @{ files = @(); partial = $false } }
function Get-DefenderHistoryInfo { @{ files = @(); partial = $false } }
foreach ($category in @('notepad_state', 'crash_dumps', 'print_spooler', 'peer_distribution_cache', 'diagnostics_timeline', 'protection_history')) {
    $result = Get-FleetForensicProjection -Category $category -FullDetail $true
    Assert-ProjectionContract $result $category
    if ($result.total -ne 0 -or @($result.records).Count -ne 0 -or @($result.columns).Count -eq 0) { throw "$category must be an empty table that keeps its columns." }
}

}
Test-Section 'phantom-row' {
# --- 3. Empty metadata categories have zero rows, never a phantom row -------
function Get-DockerDesktopDataInfo { @{ files = @(); total = 0; partial = $false; status = 'complete' } }
function Get-NetBIOSCacheInfo { @{ files = @() } }
function Get-FirewallLogInfo { @{ partial = $false } }
function Get-WERHistoryInfo { @{ files = $null; entries = $null } }
function Get-ReliabilityHistoryInfo { @{ entries = @(); files = @(); partial = $false } }
foreach ($category in @('docker_desktop_data', 'netbios_cache', 'firewall_log', 'wer_history', 'reliability_history')) {
    $result = Get-FleetForensicProjection -Category $category -FullDetail $true
    Assert-ProjectionContract $result $category
    if ($result.total -ne 0 -or @($result.records).Count -ne 0 -or $result.truncated) { throw "$category reported a phantom row for an empty source (total=$($result.total))." }
}
# An incomplete scan that found nothing is unavailable, not empty.
function Get-DockerDesktopDataInfo { @{ files = @(); total = 0; partial = $true; status = 'partial' } }
$unavailable = Get-FleetForensicProjection -Category docker_desktop_data -FullDetail $true
if (-not $unavailable.error -or $unavailable.message -notmatch '^System Cleanup could not ') { throw 'A partial scan with no rows must stay an unavailable (error) result.' }
# A populated source is unchanged.
function Get-DockerDesktopDataInfo { @{ files = @(@{ name = 'Docker WSL: disk.vhdx'; sizeKB = 2048; modified = '2026-11-20 10:00:00' }); total = 1; partial = $false } }
$populated = Get-FleetForensicProjection -Category docker_desktop_data -FullDetail $true
Assert-ProjectionContract $populated 'docker_desktop_data (populated)'
if ($populated.total -ne 1 -or $populated.records[0].sizeKB -ne 2048 -or $populated.records[0].artifact -ne 'Docker WSL: disk.vhdx') { throw 'A populated metadata category lost its row.' }

}
Test-Section 'cellular' {
# --- 4. No mobile-broadband hardware is an honest empty table ---------------
function Get-CellularHistoryInfo { @{ files = @(); total = 0; noHardware = $true; partial = $false; status = 'not_applicable'; coverageNote = 'Windows reports no mobile broadband adapter.' } }
$cellular = Get-FleetForensicProjection -Category cellular_history -FullDetail $true
Assert-ProjectionContract $cellular 'cellular_history (no hardware)'
if ($cellular.total -ne 0 -or @($cellular.records).Count -ne 0 -or $cellular.status -ne 'not_applicable' -or -not $cellular.noHardware) { throw 'No mobile broadband hardware must be an empty not-applicable table.' }
function Get-CellularHistoryInfo { @{ error = $true; message = 'Windows Mobile Broadband service is stopped; cellular profiles could not be checked.' } }
if (-not (Get-FleetForensicProjection -Category cellular_history -FullDetail $true).error) { throw 'A real cellular read failure must remain an error.' }

}
Test-Section 'amcache' {
# --- 7. An unmounted Amcache hive is an unavailable table, not a failed view
$categories = @('InventoryApplication', 'InventoryApplicationFile', 'InventoryApplicationShortcut', 'InventoryDeviceContainer', 'InventoryDriverBinary')
# Shape returned by Get-AmcacheEntries when the hive file exists but Windows
# has not mounted it (the common case) and no offline snapshot is authorised.
function Get-AmcacheEntries {
    param([switch]$FleetProjection)
    @{ entries = @($categories | ForEach-Object { @{ category = $_; count = 0; sample = @() } }); hveFileSizeMb = 8; hveFileExists = $true; inventoryRoot = $null; total = $null; partial = $true; status = 'unavailable'; message = 'App Launch Cache inventory is unavailable.'; coverage = 'limited'; coverageNote = 'Reads the available mounted Windows registry inventory only.' }
}
$unavailable = Get-FleetForensicProjection -Category amcache -FullDetail $true
Assert-ProjectionContract $unavailable 'amcache (unmounted)'
if ($unavailable.error) { throw 'An unmounted Amcache hive must not be a hard failure.' }
if (@($unavailable.records).Count -ne 5 -or $unavailable.total -ne 5 -or $unavailable.truncated) { throw 'Unavailable Amcache must list its five categories.' }
foreach ($record in @($unavailable.records)) {
    if ($record.ContainsKey('count') -or $record.ContainsKey('sample')) { throw 'Unavailable Amcache rows must omit the counts instead of reporting zero.' }
    if ($record.state -notmatch '^Unavailable: ') { throw 'Unavailable Amcache rows must say they are unavailable.' }
}
if ($unavailable.status -ne 'unavailable' -or $unavailable.hveFileExists -ne $true) { throw 'Unavailable Amcache must keep its status and hive facts.' }
$stateColumns = @($unavailable.columns | ForEach-Object { $_.key })
if (($stateColumns -join ',') -ne 'category,count,sample,state') { throw "Amcache columns changed: $($stateColumns -join ',')" }
# A readable inventory keeps its counts and is marked available.
function Get-AmcacheEntries {
    param([switch]$FleetProjection)
    @{ entries = @(@{ category = 'InventoryApplication'; count = 2; sample = @(@{ id = 'a'; name = 'App' }) }, @{ category = 'InventoryDriverBinary'; count = 0; sample = @() }); hveFileSizeMb = 8; hveFileExists = $true; inventoryRoot = 'HKLM:\Amcache\Root'; total = 2; partial = $false; status = 'complete' }
}
$readable = Get-FleetForensicProjection -Category amcache -FullDetail $true
Assert-ProjectionContract $readable 'amcache (readable)'
if ($readable.total -ne 2 -or $readable.records[0].count -ne 2 -or $readable.records[0].state -ne 'Available' -or $readable.records[1].count -ne 0) { throw 'A readable Amcache inventory lost its counts.' }
# A collector exception is still a failure.
function Get-AmcacheEntries { param([switch]$FleetProjection) @{ error = $true; message = 'boom at C:\secret' } }
$failed = Get-FleetForensicProjection -Category amcache -FullDetail $true
if (-not $failed.error -or $failed.message -notmatch '^System Cleanup could not ' -or $failed.message -match 'secret') { throw 'A real Amcache collector failure must stay a bounded error.' }

}
Test-Section 'truncation' {
# --- 6. A preview capped at the row limit reports the real total ------------
$previewFiles = @(1..200 | ForEach-Object { @{ name = "npm cache: file-$_"; sizeKB = 1; modified = '2026-11-20 10:00:00' } })
function Get-DeveloperCachesInfo { @{ files = $previewFiles; total = 350; truncated = $true; partial = $false } }
$capped = Get-FleetForensicProjection -Category developer_caches -FullDetail $true
Assert-ProjectionContract $capped 'developer_caches (capped)'
if ($capped.total -ne 350 -or @($capped.records).Count -ne 200 -or -not $capped.truncated) { throw 'A 200-row preview of 350 files must report total=350 and truncated=true.' }
function Get-DeveloperCachesInfo { @{ files = $previewFiles; total = 200; truncated = $false; partial = $false } }
$exact = Get-FleetForensicProjection -Category developer_caches -FullDetail $true
if ($exact.total -ne 200 -or $exact.truncated) { throw 'Exactly 200 files is not truncated.' }
# Even a collector that forgets to report a total cannot silently drop rows.
$overflow = New-FleetForensicProjection -Category 'dns_cache' -Label 'DNS Cache' -Columns @(@{ key = 'name'; label = 'Name'; type = 'text' }) -Records @(1..250 | ForEach-Object { @{ name = "n$_" } }) -Total 250
if (@($overflow.records).Count -ne 200 -or $overflow.total -ne 250 -or -not $overflow.truncated) { throw '250 source rows must be reported as a truncated 200-row table.' }
$understated = New-FleetForensicProjection -Category 'dns_cache' -Label 'DNS Cache' -Columns @(@{ key = 'name'; label = 'Name'; type = 'text' }) -Records @(@{ name = 'a' }, @{ name = 'b' }) -Total 1
if ($understated.total -ne 2) { throw 'A total below the carried rows must be corrected.' }
$nullRecords = New-FleetForensicProjection -Category 'dns_cache' -Label 'DNS Cache' -Columns @(@{ key = 'name'; label = 'Name'; type = 'text' }) -Records $null -Total 0
if (@($nullRecords.records).Count -ne 0 -or $nullRecords.total -ne 0 -or $nullRecords.truncated) { throw 'A null record list must produce an empty table.' }

}
Test-Section 'bounded-collectors' {
# --- 5. Bounded collectors are called with the Fleet bounds -----------------
$script:processArgs = $null
function Get-ProcessIntelligence {
    [CmdletBinding()] param([int]$MaxProcesses = 0, [int]$TimeBudgetSeconds = 0)
    $script:processArgs = @{ max = $MaxProcesses; budget = $TimeBudgetSeconds }
    @{ processes = @(1..200 | ForEach-Object { @{ name = "p$_"; pid = $_; signed = 'Valid'; signer = 'CN=Test'; elevated = 'No'; path = "C:\p$_.exe" } }); totalProcesses = 481; truncated = $true; partial = $false }
}
$review = Get-FleetForensicProjection -Category process_review -FullDetail $true
Assert-ProjectionContract $review 'process_review'
if ($script:processArgs.max -ne 200 -or $script:processArgs.budget -le 0 -or $script:processArgs.budget -ge 60) { throw 'Process review must be bounded by the row limit and a budget under the 60 s collector timeout.' }
if ($review.total -ne 481 -or -not $review.truncated -or @($review.records).Count -ne 200) { throw 'Process review must report the full process count as truncated.' }
$script:shellArgs = $null
function Get-ShellBags {
    [CmdletBinding()] param([int]$MaxEntries = 0, [int]$TimeBudgetSeconds = 0)
    $script:shellArgs = @{ max = $MaxEntries; budget = $TimeBudgetSeconds }
    @{ entries = @(1..200 | ForEach-Object { @{ path = "Folder view ($_)"; lastModified = ''; source = 'ShellBags' } }); total = 43000; truncated = $false; partial = $false }
}
$bags = Get-FleetForensicProjection -Category shell_bags -FullDetail $true
Assert-ProjectionContract $bags 'shell_bags'
if ($script:shellArgs.max -ne 200 -or $script:shellArgs.budget -le 0 -or $script:shellArgs.budget -ge 60) { throw 'ShellBags must be bounded by the row limit and a budget under the 60 s collector timeout.' }
if ($bags.total -ne 43000 -or -not $bags.truncated) { throw 'ShellBags must report the full key count and mark the table truncated.' }

}
Test-Section 'shellbags-walk' {
# --- ShellBags walk shape (read-only registry reads on this account) --------
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-ShellBags' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$walkParameters = (Get-Command Get-ShellBags).Parameters.Keys
if ('MaxEntries' -notin $walkParameters -or 'TimeBudgetSeconds' -notin $walkParameters) { throw 'Get-ShellBags must expose an entry cap and a time budget instead of always walking the whole tree.' }
$timer = [System.Diagnostics.Stopwatch]::StartNew()
$walk = Get-ShellBags -MaxEntries 5 -TimeBudgetSeconds 20
if ($walk.error -or @($walk.entries).Count -gt 5 -or $walk.total -lt @($walk.entries).Count) { throw 'ShellBags walk shape is wrong.' }
if ($timer.Elapsed.TotalSeconds -gt 25) { throw "Bounded ShellBags walk took $($timer.Elapsed.TotalSeconds) s." }
foreach ($entry in @($walk.entries)) { if ($entry.source -ne 'ShellBags' -or [string]::IsNullOrWhiteSpace($entry.path)) { throw 'A ShellBags entry lost its fields.' } }

}
if ($script:failures.Count) { throw ($script:failures -join [Environment]::NewLine) }
'PASS: Fleet forensic projections: supported column types, empty sources, phantom rows, not-applicable cellular, unavailable Amcache, truncation and bounded collectors.'
