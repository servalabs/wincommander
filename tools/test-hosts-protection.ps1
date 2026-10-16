$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$moduleRoot = Join-Path $repo 'src-tauri\commander-free\scripts\modules\network'
foreach ($module in @('blocklists-data', 'hosts-state', 'hosts-verification', 'hosts', 'connection-check')) { . (Join-Path $moduleRoot ($module + '.ps1')) }
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ('wincom-hosts-test-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($testRoot)
$script:HostsFilePath = Join-Path $testRoot 'hosts'
$script:HostsMutexName = 'Local\WinComHostsTest-' + [guid]::NewGuid().ToString('N')
$script:denyAdmin = $false
$script:dnsFails = $false
function Assert-IsAdmin { if ($script:denyAdmin) { throw 'Administrator privileges required.' } }
function Clear-DnsClientCache { [CmdletBinding()]param(); if ($script:dnsFails) { throw 'Mocked DNS failure.' } }
function Assert-True { param([bool]$Value, [string]$Message); if (-not $Value) { throw $Message } }
try {
    $original = "# Personal entries`r`n127.0.0.1 localhost`r`n192.0.2.7 internal.example # keep`r`n"
    [IO.File]::WriteAllText($script:HostsFilePath, $original)
    $status = Get-BlocklistStatus
    Assert-True (-not $status.error -and $status.applied.Count -eq 0) 'Initial status must be readable without modifying the hosts file.'
    Assert-True (-not [IO.File]::Exists($script:HostsFilePath + '.wincommander-undo.json')) 'Read-only status must not create recovery state.'

    $data = Get-HardcodedBlocklists
    $telemetry = Get-HostsRecords $data['telemetry-blocklist']
    Assert-True (@(Get-HostsEssentialOverrides $telemetry).Count -eq 0) 'Telemetry defaults must exclude shared security, update and connectivity destinations.'
    foreach ($domain in @('sentry.io', 'segment.io', 'amplitude.com', 'datadoghq.com', 'mixpanel.com')) {
        Assert-True (-not $telemetry.ContainsKey($domain)) "Broad service portal $domain must not be blocked."
    }

    $result = Add-BlocklistToHosts 'telemetry-blocklist'
    Assert-True ($result.verified -and $result.changed) ('Enabling protection must verify the real hosts readback: ' + ($result | ConvertTo-Json -Compress))
    $status = Get-BlocklistStatus
    $state = @($status.available | Where-Object name -eq 'telemetry-blocklist')[0].verification
    Assert-True ($state.state -eq 'verified' -and $status.undo.available) 'Applied entries and undo must be verified.'
    $firstReceipt = [IO.File]::ReadAllText($script:HostsFilePath + '.wincommander-undo.json')
    $result = Add-BlocklistToHosts 'telemetry-blocklist'
    Assert-True ($result.verified -and -not $result.changed) 'Reapplying current defaults must be a no-op.'
    Assert-True ($firstReceipt -ceq [IO.File]::ReadAllText($script:HostsFilePath + '.wincommander-undo.json')) 'A no-op must preserve the previous undo.'
    $external = "`r`n192.0.2.8 later.example # keep later edits`r`n"
    [IO.File]::AppendAllText($script:HostsFilePath, $external)
    $result = Undo-HostsBlocklistChange
    Assert-True $result.verified 'Undo should succeed when only unrelated entries changed.'
    Assert-True ([IO.File]::ReadAllText($script:HostsFilePath).Contains($external)) 'Undo must preserve edits outside the managed section.'
    Assert-True ([IO.File]::ReadAllText($script:HostsFilePath).StartsWith($original)) 'Undo must preserve original content exactly.'
    Assert-True (-not (Get-BlocklistStatus).undo.available) 'Undo must be single-use.'

    [void](Add-BlocklistToHosts 'ai-sites')
    $beforeBatch = Read-HostsContent
    $batch = Set-HostsRecommendedProtection $true
    Assert-True $batch.verified 'Recommended protection must apply as one verified change.'
    [void](Undo-HostsBlocklistChange)
    Assert-True ((Read-HostsContent) -ceq $beforeBatch) 'Undo must restore the entire recommended action while preserving an existing content list.'
    $batch = Set-HostsRecommendedProtection $false
    Assert-True $batch.verified 'Recommended protection must also accept a false boolean.'
    Assert-True (@(Get-AppliedBlocklistsFromContent (Read-HostsContent)) -contains 'ai-sites') 'Turning off recommended protection must preserve content lists.'

    [void](Remove-BlocklistFromHosts 'ai-sites')
    $savedWriter = ${function:Write-HostsAtomicFile}
    function Write-HostsAtomicFile {
        param([string]$Path, [string]$Content)
        if ($Path -eq $script:HostsFilePath) { return }
        & $savedWriter $Path $Content
    }
    $beforeNoopWrite = Read-HostsContent
    $priorReceipt = [IO.File]::ReadAllText($script:HostsFilePath + '.wincommander-undo.json')
    Assert-True (Add-BlocklistToHosts 'telemetry-blocklist').error 'A setter that does not change the hosts file must fail independent readback.'
    Assert-True ((Read-HostsContent) -ceq $beforeNoopWrite) 'Failed readback must preserve existing content.'
    Assert-True ($priorReceipt -ceq [IO.File]::ReadAllText($script:HostsFilePath + '.wincommander-undo.json')) 'An unsuccessful write must preserve the last successful undo.'
    Set-Item -Path function:Write-HostsAtomicFile -Value $savedWriter

    [void](Add-BlocklistToHosts 'telemetry-blocklist')
    $applied = Read-HostsContent
    [IO.File]::WriteAllText($script:HostsFilePath, $applied.Replace('0.0.0.0 api.sentry.io', '192.0.2.4 api.sentry.io'))
    $status = Get-BlocklistStatus
    $state = @($status.available | Where-Object name -eq 'telemetry-blocklist')[0].verification
    Assert-True ($state.state -eq 'partial') 'An entry redirected to a real address must not verify as blocked.'
    $beforeUndo = Read-HostsContent
    Assert-True (Undo-HostsBlocklistChange).error 'Undo must refuse later changes inside its managed section.'
    Assert-True ((Read-HostsContent) -ceq $beforeUndo) 'Refused undo must not change the file.'

    [IO.File]::WriteAllText($script:HostsFilePath, $applied.Replace('# --- telemetry-blocklist ---', "# --- telemetry-blocklist ---`r`n0.0.0.0 winatp-gw-cus.microsoft.com`r`n0.0.0.0 us-v20.events.data.microsoft.com"))
    $state = @((Get-BlocklistStatus).available | Where-Object name -eq 'telemetry-blocklist')[0].verification
    Assert-True ($state.state -eq 'attention') 'Legacy security endpoints must require a protection update.'
    [void](Add-BlocklistToHosts 'telemetry-blocklist')
    Assert-True ((Read-HostsContent) -notmatch 'winatp-gw-cus') 'Updating protection must remove legacy security blocks.'

    [IO.File]::AppendAllText($script:HostsFilePath, "`r`n192.0.2.5 api.sentry.io`r`n")
    $state = @((Get-BlocklistStatus).available | Where-Object name -eq 'telemetry-blocklist')[0].verification
    Assert-True ($state.state -eq 'partial') 'External conflicting hosts aliases must prevent a verified status.'
    [IO.File]::WriteAllText($script:HostsFilePath, $original)
    $script:dnsFails = $true
    $result = Add-BlocklistToHosts 'telemetry-blocklist'
    Assert-True ($result.verified -and $result.dnsWarning) 'DNS cache failures must be visible after a verified write.'
    $script:dnsFails = $false

    $script:denyAdmin = $true
    $beforeDenied = Read-HostsContent
    foreach ($operation in @({ Add-BlocklistToHosts 'telemetry-blocklist' }, { Remove-BlocklistFromHosts 'telemetry-blocklist' }, { Set-HostsRecommendedProtection $true }, { Undo-HostsBlocklistChange })) {
        $denied = $false
        try { & $operation | Out-Null } catch { $denied = $true }
        Assert-True $denied 'All mutations must require administrator rights.'
    }
    Assert-True ((Read-HostsContent) -ceq $beforeDenied) 'Denied actions must not write.'
    $script:denyAdmin = $false

    $beforeBadName = Read-HostsContent
    Assert-True (Add-BlocklistToHosts '../other').error 'Only backend-owned list identifiers may be selected.'
    Assert-True ((Read-HostsContent) -ceq $beforeBadName) 'An invalid list must not write.'
    [IO.File]::AppendAllText($script:HostsFilePath, "`r`n# WINCOMMANDER_BLOCKLIST_START`r`n")
    Assert-True (Get-BlocklistStatus).error 'Duplicated or malformed sections must fail visibly.'
    Assert-True (Remove-BlocklistFromHosts 'telemetry-blocklist').error 'Malformed sections must refuse writes.'

    Write-Output 'Hosts protection passed: safe defaults, real readback, conflicting entries, stale defaults, single-use undo, preserved edits, no-op recovery, DNS failure and administrator enforcement.'
} finally {
    # Delete only the uniquely created test directory after checking its resolved location.
    $resolved = [IO.Path]::GetFullPath($testRoot)
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if ($resolved.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -and (Split-Path $resolved -Leaf) -like 'wincom-hosts-test-*') {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
