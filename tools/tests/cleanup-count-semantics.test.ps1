$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $path), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup source did not parse.' }
foreach ($name in @('Get-EventLogSummary', 'Get-SRUMData', 'Get-AppLaunchHistoryInfo', 'Get-SearchIndexInfo')) {
    $definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $definition) { throw "Missing reader: $name" }
    . ([scriptblock]::Create($definition.Extent.Text))
}
$script:mode = 'populated'; $script:paths = @()
function Get-WinEvent {
    [CmdletBinding()] param([string]$ListLog)
    if ($script:mode -eq 'denied') { throw [UnauthorizedAccessException]::new('Denied') }
    if ($script:mode -eq 'empty') { return }
    [pscustomobject]@{ LogName='Application'; RecordCount=11; FileSize=1024; IsEnabled=$true }
    [pscustomobject]@{ LogName='System'; RecordCount=4; FileSize=2048; IsEnabled=$true }
    if ($script:mode -eq 'partial') { Write-Error 'Cannot read Security' }
    if ($script:mode -eq 'unknown') { [pscustomobject]@{ LogName='Security'; RecordCount=$null; FileSize=0; IsEnabled=$true } }
}
function Get-Service {
    [CmdletBinding()] param([string]$Name)
    [pscustomobject]@{ Status='Running' }
}
function Get-Item {
    [CmdletBinding()] param([string]$LiteralPath, [switch]$Force)
    $script:paths += $LiteralPath
    if ($script:mode -eq 'denied') { throw [UnauthorizedAccessException]::new('Denied') }
    if ($script:mode -eq 'empty') { throw [System.Management.Automation.ItemNotFoundException]::new('Missing') }
    if ($script:mode -eq 'lockedSid' -and $LiteralPath -eq 'MockSid2') { throw [UnauthorizedAccessException]::new('Denied SID') }
    if ($LiteralPath -eq 'MockSid1') { return [pscustomobject]@{ Property=@('Version', 'SequenceNumber', '\Device\Mock\a.exe', '\Device\Mock\b.exe') } }
    if ($LiteralPath -eq 'MockSid2') { return [pscustomobject]@{ Property=@('\Device\Mock\c.exe') } }
    [pscustomobject]@{ Name=($LiteralPath -split '\\')[-1]; FullName=$LiteralPath; Length=0; LastWriteTime=[DateTime]'2026-10-01' }
}
function Get-ChildItem {
    [CmdletBinding()] param([string]$LiteralPath, [switch]$File, [switch]$Force, [switch]$Recurse, [string]$Filter)
    $script:paths += "$LiteralPath|$Filter|$Recurse"
    if ($LiteralPath -like '*bam*UserSettings') {
        [pscustomobject]@{ PSPath='MockSid1'; PSChildName='S-1-5-21-Mock1' }
        [pscustomobject]@{ PSPath='MockSid2'; PSChildName='S-1-5-21-Mock2' }
        return
    }
    if ($script:mode -eq 'deniedChild') { throw [UnauthorizedAccessException]::new('Denied enumeration') }
    $name = if ($Filter) { 'Windows.db' } else { 'Gather.log' }
    [pscustomobject]@{ Name=$name; FullName="$LiteralPath\$name"; Length=1024; LastWriteTime=[DateTime]'2026-10-01' }
}
$events = Get-EventLogSummary
if ($events.error -or $events.total -ne 15 -or $events.logCount -ne 2 -or ($events.logs | ForEach-Object { $_.count } | Measure-Object -Sum).Sum -ne 15) { throw 'Event records were confused with channel rows.' }
$script:mode = 'partial'
$partial = Get-EventLogSummary
if (-not $partial.partial -or $partial.complete -or $partial.total -ne 15) { throw 'Unreadable event channels were reported as a complete scan.' }
$script:mode = 'unknown'
if (-not (Get-EventLogSummary).partial) { throw 'Unknown enabled-channel count was treated as zero.' }
$script:mode = 'populated'
$srum = Get-SRUMData
if ($srum.error -or $srum.total -ne 1 -or $srum.files.Count -ne 1 -or -not $srum.databaseExists -or $srum.historicalRecordsAvailable -or $srum.srumSizeMb -ne 0) { throw 'SRUM file existence was confused with usage records or zero-byte absence.' }
if ($srum.ContainsKey('entries')) { throw 'SRUM fabricated historical entries from live processes.' }
$bam = Get-AppLaunchHistoryInfo
if ($bam.error -or $bam.total -ne 3 -or $bam.files.Count -ne 3 -or @($bam.files | Where-Object { $_.name -in @('Version', 'SequenceNumber') }).Count) { throw 'BAM tracked programs were confused with user summaries or control values.' }
$script:mode = 'lockedSid'
if (-not (Get-AppLaunchHistoryInfo).error) { throw 'An unreadable BAM SID was reported as complete.' }
$script:mode = 'populated'; $script:paths = @()
$search = Get-SearchIndexInfo
if ($search.error -or $search.total -ne 4 -or $search.files.Count -ne 4) { throw 'Search target file counts were wrong.' }
if (@($script:paths | Where-Object { $_ -like '*\Data\Temp*' }).Count) { throw 'Search reader included files outside its clearer scope.' }
if (-not @($script:paths | Where-Object { $_ -like '*|Windows*.db*|False' }).Count) { throw 'Search root databases were not scoped to Windows*.db*.' }
$script:mode = 'deniedChild'
if (-not (Get-SearchIndexInfo).error) { throw 'An unreadable Search subtree was reported as complete.' }
foreach ($mode in @('empty', 'denied')) {
    $script:mode = $mode
    foreach ($reader in @('Get-EventLogSummary', 'Get-SRUMData', 'Get-AppLaunchHistoryInfo', 'Get-SearchIndexInfo')) {
        $result = & $reader
        if ($mode -eq 'empty' -and ($result.error -or $result.total -ne 0)) { throw "$reader did not report a successful empty scan." }
        if ($mode -eq 'denied' -and -not $result.error) { throw "$reader reported denied data as zero." }
    }
}
Write-Output 'Passed: EventLog record totals and partial coverage; SRUM store-only counts; BAM records; matching Search scope; empty and denied cases.'
