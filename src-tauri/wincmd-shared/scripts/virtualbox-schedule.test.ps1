$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')
. ([scriptblock]::Create($script:ScheduledVmFunctions))

function Assert-WCVmTest($Condition, $Message) { if (-not $Condition) { throw $Message } }
$powershell = Join-Path $PSHOME 'powershell.exe'
$clock = [Diagnostics.Stopwatch]::StartNew()
$timedOut = $false
try { $null = Invoke-WCProviderProcess -FilePath $powershell -Arguments @('-NoProfile', '-Command', 'Start-Sleep -Seconds 5') -TimeoutSeconds 1 }
catch { $timedOut = $_.Exception.Message -match 'did not finish in time' }
$clock.Stop()
Assert-WCVmTest ($timedOut -and $clock.Elapsed.TotalSeconds -lt 15) 'Read-only provider calls must fail within their timeout.'
$script:vmProcessNames = @('SyntheticProvider')
function Get-Process { param($Name, $ErrorAction) return @($script:vmProcessNames | ForEach-Object { [pscustomobject]@{ ProcessName = $_ } }) }
$busy = $false
try { Assert-WCProviderIdle -Name 'SyntheticProvider' } catch { $busy = $true }
Assert-WCVmTest $busy 'A previous provider operation must prevent a conflicting retry.'
$Error.Clear()
Assert-WCProviderIdle -Name 'AbsentProvider'
Assert-WCVmTest ($Error.Count -eq 0) 'An absent idle provider must not leave a suppressed error that marks cleanup incomplete.'
$lines = @('Unexpected localized inventory output')
function Invoke-WCVBox { param([string[]]$Arguments) return $lines }
$unknown = $false
try { $null = Get-WCVBoxSnapshots '11111111-1111-1111-1111-111111111111' } catch { $unknown = $true }
Assert-WCVmTest $unknown 'Unrecognized localized snapshot output must remain unverified.'
$vm1 = '11111111-1111-1111-1111-111111111111'
$vm2 = '22222222-2222-2222-2222-222222222222'
$vm3 = '33333333-3333-3333-3333-333333333333'
$snap1 = 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'
$snap2 = 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb'
$script:snapshots = @{ $vm1 = @($snap1, $snap2); $vm2 = @($snap1); $vm3 = @($snap1) }
$script:deletions = [Collections.Generic.List[string]]::new()
$script:scenario = 'normal'
$script:vm1InfoReads = 0

function Invoke-WCVBox {
    param([string[]]$Arguments)
    if ($Arguments[0] -eq 'list') {
        if ($script:scenario -eq 'badMachine') { return 'unexpected provider output' }
        return @(('"first" {' + $vm1 + '}'), ('"second" {' + $vm2 + '}'), ('"third" {' + $vm3 + '}'))
    }
    if ($Arguments[0] -eq 'showvminfo') {
        $id = $Arguments[1]
        if ($id -eq $vm1) { $script:vm1InfoReads++ }
        if ($id -eq $vm1 -and $script:scenario -eq 'started' -and $script:vm1InfoReads -gt 1) { return @('CfgFile="C:\test\inside.vbox"', 'VMState="running"') }
        if ($id -eq $vm1) { return @('CfgFile="C:\test\inside.vbox"', 'VMState="poweroff"') }
        if ($id -eq $vm2) { return @('CfgFile="C:\test\outside.vbox"', 'VMState="poweroff"') }
        return @('CfgFile="C:\test\inside-running.vbox"', 'VMState="running"')
    }
    if ($Arguments[0] -eq 'snapshot' -and $Arguments[2] -eq 'list') {
        if ($script:scenario -eq 'badSnapshot') { return 'SnapshotName="missing-uuid"' }
        $lines = @(); $i = 0
        foreach ($id in $script:snapshots[$Arguments[1]]) { $i++; $lines += "SnapshotName-$i=`"Snapshot $i`""; $lines += "SnapshotUUID-$i=`"$id`"" }
        return $lines
    }
    if ($Arguments[0] -eq 'snapshot' -and $Arguments[2] -eq 'delete') {
        $script:deletions.Add($Arguments[1] + ':' + $Arguments[3])
        if ($script:scenario -ne 'retained') { $script:snapshots[$Arguments[1]] = @($script:snapshots[$Arguments[1]] | Where-Object { $_ -ne $Arguments[3] }) }
        return @()
    }
    throw 'Unexpected provider call.'
}
function Test-WCVBoxMachineInScope($ConfigPath) { return $ConfigPath -like '*inside*' }

$result = Invoke-WCVBoxSnapshotCleanup
Assert-WCVmTest ($result.removed -eq 2) 'Each snapshot must be removed and read back.'
Assert-WCVmTest ($result.failed -ge 1) 'A running VM must be reported as incomplete.'
Assert-WCVmTest ($script:deletions.Count -eq 2 -and @($script:deletions | Where-Object { $_ -notlike "$vm1*" }).Count -eq 0) 'Out-of-scope and running VMs must not be mutated.'

$script:scenario = 'badMachine'; $script:deletions.Clear()
$result = Invoke-WCVBoxSnapshotCleanup
Assert-WCVmTest ($result.failed -gt 0 -and $script:deletions.Count -eq 0) 'Invalid machine identities must fail closed.'

$script:scenario = 'badSnapshot'; $script:deletions.Clear()
$result = Invoke-WCVBoxSnapshotCleanup
Assert-WCVmTest ($result.failed -gt 0 -and $script:deletions.Count -eq 0) 'Incomplete snapshot inventory must not delete.'

$script:scenario = 'retained'; $script:snapshots[$vm1] = @($snap1); $script:deletions.Clear()
$result = Invoke-WCVBoxSnapshotCleanup
Assert-WCVmTest ($result.failed -gt 0 -and $result.removed -eq 0) 'A provider that leaves a snapshot present must report failure.'

$script:scenario = 'started'; $script:vm1InfoReads = 0; $script:snapshots[$vm1] = @($snap1); $script:deletions.Clear()
$result = Invoke-WCVBoxSnapshotCleanup
Assert-WCVmTest ($result.failed -gt 0 -and $script:deletions.Count -eq 0) 'A VirtualBox machine that starts after inventory must not have snapshots deleted.'

$savedVmPathCheck = ${function:Get-WCVmPathIfPresent}
$savedFileInventory = ${function:Get-AutoEraseFiles}
try {
    $script:vmInventoryMode = 'normal'
    function Get-WCVmPathIfPresent {
        param($Path, $Directory)
        if ($script:vmInventoryMode -eq 'missing') { return $null }
        if ($script:vmInventoryMode -eq 'denied') { throw 'Synthetic VMware root denial.' }
        if ($script:vmInventoryMode -eq 'reparse') { throw 'Synthetic redirected VMware root.' }
        return [pscustomobject]@{ PSIsContainer = $true }
    }
    function Get-AutoEraseFiles {
        param($Path, $Filter, [switch]$Recurse)
        if ($Filter -ne '*.vmx' -or -not $Recurse) { throw 'VMware discovery must use the verified recursive inventory.' }
        return @([pscustomobject]@{ FullName = 'C:\fixture\one.vmx' }, [pscustomobject]@{ FullName = 'C:\fixture\nested\two.vmx' })
    }
    Assert-WCVmTest (@(Get-WCVmwareMachines).Count -eq 2) 'VMware inventory must include verified nested machine files.'
    $script:vmInventoryMode = 'missing'
    Assert-WCVmTest (@(Get-WCVmwareMachines).Count -eq 0) 'An absent VMware root must be an empty inventory.'
    foreach ($mode in @('denied', 'reparse')) {
        $script:vmInventoryMode = $mode; $failed = $false
        try { $null = Get-WCVmwareMachines } catch { $failed = $true }
        Assert-WCVmTest $failed 'An inaccessible or redirected VMware root must not be reported empty.'
    }
} finally {
    Set-Item Function:\Get-WCVmPathIfPresent -Value $savedVmPathCheck
    Set-Item Function:\Get-AutoEraseFiles -Value $savedFileInventory
}

function Get-WCVmwareMachines { return @('C:\fixture\powered-off.vmx', 'C:\fixture\running.vmx') }
function Test-WCVmwareMachineInScope($ConfigPath) { return $true }
$script:vmwareSnapshots = @{ 'C:\fixture\powered-off.vmx' = @('snapshot A', 'snapshot B'); 'C:\fixture\running.vmx' = @('running-snapshot') }
$script:vmwareDeletes = [Collections.Generic.List[string]]::new()
$script:vmwareScenario = 'normal'
$script:vmwareListReads = 0
function Invoke-WCVmrun {
    param([string[]]$Arguments)
    if ($Arguments[0] -eq 'list') {
        $script:vmwareListReads++
        if ($script:vmwareScenario -eq 'started' -and $script:vmwareListReads -gt 1) { return @('Total running VMs: 2', 'C:\fixture\running.vmx', 'C:\fixture\powered-off.vmx') }
        return @('Total running VMs: 1', 'C:\fixture\running.vmx')
    }
    if ($Arguments[0] -eq 'listSnapshots') {
        if ($script:vmwareScenario -eq 'malformed') { return @('Total snapshots: 2', 'one') }
        $snapshots = @($script:vmwareSnapshots[$Arguments[1]])
        return @("Total snapshots: $($snapshots.Count)") + $snapshots
    }
    if ($Arguments[0] -eq 'deleteSnapshot') {
        $script:vmwareDeletes.Add($Arguments[1] + ':' + $Arguments[2])
        if ($script:vmwareScenario -ne 'retained') { $script:vmwareSnapshots[$Arguments[1]] = @($script:vmwareSnapshots[$Arguments[1]] | Where-Object { $_ -cne $Arguments[2] }) }
        return @()
    }
    throw 'Unexpected provider call.'
}
$result = Invoke-WCVmwareSnapshotCleanup
Assert-WCVmTest ($result.removed -eq 2 -and $result.failed -gt 0) 'VMware must delete powered-off snapshots and report the running machine.'
Assert-WCVmTest ($script:vmwareDeletes.Count -eq 2 -and @($script:vmwareDeletes | Where-Object { $_ -like '*running.vmx*' }).Count -eq 0) 'A running VMware machine must not be changed.'
$script:vmwareScenario = 'malformed'; $script:vmwareDeletes.Clear()
$result = Invoke-WCVmwareSnapshotCleanup
Assert-WCVmTest ($result.failed -gt 0 -and $script:vmwareDeletes.Count -eq 0) 'An inconsistent VMware inventory must fail closed.'
$script:vmwareScenario = 'retained'; $script:vmwareSnapshots['C:\fixture\powered-off.vmx'] = @('retained'); $script:vmwareDeletes.Clear()
$result = Invoke-WCVmwareSnapshotCleanup
Assert-WCVmTest ($result.failed -gt 0 -and $result.removed -eq 0) 'VMware readback must detect snapshots that remain.'
$script:vmwareScenario = 'normal'; $script:vmwareSnapshots['C:\fixture\powered-off.vmx'] = @('same', 'same'); $script:vmwareDeletes.Clear()
$result = Invoke-WCVmwareSnapshotCleanup
Assert-WCVmTest ($result.failed -gt 0 -and $script:vmwareDeletes.Count -eq 0) 'Ambiguous VMware snapshot names must not be deleted.'
$script:vmwareScenario = 'started'; $script:vmwareListReads = 0; $script:vmwareSnapshots['C:\fixture\powered-off.vmx'] = @('snapshot'); $script:vmwareDeletes.Clear()
$result = Invoke-WCVmwareSnapshotCleanup
Assert-WCVmTest ($result.failed -gt 0 -and $script:vmwareDeletes.Count -eq 0) 'A VMware machine that starts after inventory must not have snapshots deleted.'

function Get-Item {
    param($LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:vmItemMode -eq 'missing') { throw [Management.Automation.ItemNotFoundException]::new('Synthetic absent path.') }
    if ($script:vmItemMode -eq 'denied') { throw [UnauthorizedAccessException]::new('Synthetic access denial.') }
    if ($LiteralPath -like 'HKCU:*') { return [pscustomobject]@{ Property = @($script:vmRecentNames) } }
    if ($script:vmItemMode -eq 'reparse') { return [pscustomobject]@{ PSIsContainer=$true; Attributes=[IO.FileAttributes]::ReparsePoint } }
    return [pscustomobject]@{ PSIsContainer=$true; Attributes=[IO.FileAttributes]::Directory }
}
function Remove-ItemProperty {
    param($LiteralPath, $Name, $ErrorAction)
    if ($script:vmRemoveDenied) { throw [UnauthorizedAccessException]::new('Synthetic registry denial.') }
    $script:vmRecentNames = @($script:vmRecentNames | Where-Object { $_ -ne $Name })
}
$script:vmItemMode = 'missing'
Assert-WCVmTest ($null -eq (Get-WCVmPathIfPresent -Path 'C:\fixture\missing' -Directory $true)) 'Missing VM paths should be absent.'
$script:vmItemMode = 'denied'; $denied = $false
try { $null = Get-WCVmPathIfPresent -Path 'C:\fixture\denied' -Directory $true } catch { $denied = $true }
Assert-WCVmTest $denied 'Denied VM paths must not be treated as absent.'
$script:vmItemMode = 'reparse'; $reparse = $false
try { $null = Get-WCVmPathIfPresent -Path 'C:\fixture\reparse' -Directory $true } catch { $reparse = $true }
Assert-WCVmTest $reparse 'Reparse VM paths must not be treated as verified.'
$script:vmItemMode = 'normal'; $script:vmRemoveDenied = $false; $script:vmRecentNames = @('mru1', 'recentPath', 'KeepSetting')
$recent = Remove-WCVmwareRecentHistory
Assert-WCVmTest ($recent.removed -eq 2 -and $recent.failed -eq 0 -and @($script:vmRecentNames).Count -eq 1 -and $script:vmRecentNames[0] -eq 'KeepSetting') 'Only VMware recent-history properties should be removed and verified.'
$script:vmRemoveDenied = $true; $script:vmRecentNames = @('mru1')
$recent = Remove-WCVmwareRecentHistory
Assert-WCVmTest ($recent.failed -gt 0 -and $recent.removed -eq 0) 'Registry access denial must remain incomplete.'

$script:vmLogs = @('C:\fixture\vmware.log')
function Get-WCVmUserLogFiles { return @($script:vmLogs) }
function Erase-OneFile { param($Path) $script:vmLogs = @($script:vmLogs | Where-Object { $_ -ne $Path }); $script:AutoEraseRemoved++; return $true }
$script:vmProcessNames = @('vmware-vmx'); $logBlocked = $false; $script:AutoEraseRemoved = 0
try { Invoke-WCVmUserLogCleanup } catch { $logBlocked = $true }
Assert-WCVmTest ($logBlocked -and $script:vmLogs.Count -eq 1 -and $script:AutoEraseRemoved -eq 0) 'A running VM must keep its logs untouched.'
$script:vmProcessNames = @()
Invoke-WCVmUserLogCleanup
Assert-WCVmTest ($script:vmLogs.Count -eq 0 -and $script:AutoEraseRemoved -eq 1) 'Stopped-machine user logs must be erased and verified.'

function Get-WCVmPathIfPresent { param($Path, $Directory) return [pscustomobject]@{} }
function Invoke-WCVBoxSnapshotCleanup { return @{ removed = 2; failed = 0 } }
function Invoke-WCVmwareSnapshotCleanup { return @{ removed = 0; failed = 0 } }
function Remove-WCVmwareRecentHistory { return @{ removed = 0; failed = 0 } }
function Get-WCVirtualMachineArtifactRemainders { return 3 }
$result = Invoke-WCVirtualMachineArtifactCleanup
Assert-WCVmTest ($result.removed -eq 2 -and $result.failed -gt 0) 'Remaining unmanaged artifacts must prevent a complete result.'
function Get-WCVirtualMachineArtifactRemainders { throw 'Synthetic inventory failure' }
$result = Invoke-WCVirtualMachineArtifactCleanup
Assert-WCVmTest ($result.failed -gt 0) 'A failed remaining-artifact inventory must not report success.'
$script:AutoEraseRemoved = 0; $script:AutoEraseFailed = 0
Invoke-Expression $script:AutoEraseScripts['virtualMachineArtifacts']
Assert-WCVmTest ($script:AutoEraseRemoved -eq 2 -and $script:AutoEraseFailed -gt 0) 'The scheduled payload must preserve an incomplete provider result.'
$capabilities = Get-AutoEraseSupportedCategories
Assert-WCVmTest ('virtualMachineArtifacts' -in $capabilities.schedulableCategories -and 'virtualMachineArtifacts' -notin $capabilities.categories) 'VM cleanup must support individual schedules without bulk auto-set.'
Assert-WCVmTest ('virtualMachineArtifacts' -in $script:AutoEraseHourlyCategories -and 'virtualMachineArtifacts' -notin $script:AutoEraseSystemCategories) 'VM schedules must be user-scoped and no more frequent than hourly.'
$unconfirmed = Set-AutoEraseScheduleCore -CategoryId 'virtualMachineArtifacts' -IntervalMinutes 60
Assert-WCVmTest $unconfirmed.error 'VM schedules must require explicit per-card authorization.'
$fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) ('wincommander-vm-schedule-test-' + [guid]::NewGuid())
$null = New-Item -ItemType Directory -Path $fixtureRoot -ErrorAction Stop
function Initialize-AutoEraseStorage { return @{ scripts = $fixtureRoot } }
try {
    $null = ConvertTo-AutoEraseTaskArgument -CategoryId 'virtualMachineArtifacts' -IntervalMinutes 60 -Script $script:AutoEraseScripts['virtualMachineArtifacts'] -ExecutionScope 'fixture-user' -FirstDueUtc ([DateTime]::UtcNow.ToString('o')) -AllowDefaultExcluded $true
    $generatedItems = @(Get-ChildItem -LiteralPath $fixtureRoot -Filter 'virtualMachineArtifacts.fixture-user.g*.ps1' -File)
    Assert-WCVmTest ($generatedItems.Count -eq 1) 'Exactly one generated VM task must exist.'
    $generated = $generatedItems[0].FullName
    $source = [IO.File]::ReadAllText($generated)
    Assert-WCVmTest ($source.Contains('function Invoke-WCVirtualMachineArtifactCleanup') -and $source.Contains('Invoke-WCVirtualMachineArtifactCleanup')) 'Scheduled script must embed the VM provider and action.'
    $tokens = $null; $errors = $null
    $null = [Management.Automation.Language.Parser]::ParseFile($generated, [ref]$tokens, [ref]$errors)
    Assert-WCVmTest ($errors.Count -eq 0) 'Scheduled VM script must parse without errors.'
} finally {
    $absolute = [IO.Path]::GetFullPath($fixtureRoot)
    $temp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if (-not $absolute.StartsWith($temp, [StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($absolute) -notlike 'wincommander-vm-schedule-test-*') { throw 'Unexpected test fixture path.' }
    Remove-Item -LiteralPath $absolute -Recurse -Force -ErrorAction Stop
}
Write-Output 'Passed: VirtualBox and VMware provider-owned snapshot deletion, scope, running VM, identity and readback fixtures.'
