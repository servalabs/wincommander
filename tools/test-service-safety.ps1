$ErrorActionPreference = 'Stop'
$modules = Join-Path (Split-Path -Parent $PSScriptRoot) 'src-tauri\commander-free\scripts\modules\tweaks'
foreach ($module in @('service-store', 'service-snapshots', 'service-plan', 'service-execution', 'maintenance', 'service-manager')) {
    . (Join-Path $modules "$module.ps1")
}
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ('wincom-service-tests-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($testRoot)
$testAcl = New-Object Security.AccessControl.DirectorySecurity
$testAcl.SetAccessRuleProtection($true, $false)
$testAcl.SetOwner([Security.Principal.SecurityIdentifier]::new('S-1-5-32-544'))
foreach ($sid in @('S-1-5-18', 'S-1-5-32-544')) {
    [void]$testAcl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new(
        [Security.Principal.SecurityIdentifier]::new($sid), 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow'))
}
Set-Acl -LiteralPath $testRoot -AclObject $testAcl
function Assert-IsAdmin {}
function Get-ServiceSafetyRoot { return $testRoot }
function Get-ServiceSafetyMachine { return 'test-machine' }
$script:world = @()
$script:writes = [Collections.Generic.List[string]]::new()
function New-TestService([string]$Name, [string[]]$Dependencies = @(), [string]$State = 'Running', [string]$Mode = 'Auto') {
    return [pscustomobject][ordered]@{
        name = $Name; displayName = "$Name service"; startMode = $Mode; delayedAutoStart = $false; state = $State
        account = 'LocalSystem'; binaryPath = "C:\Services\$Name.exe"; dependencies = @($Dependencies)
        externalDependencies = @(); externalDependents = @(); canStop = ($State -eq 'Running'); perUserInstance = $false
    }
}
function Get-ServiceSafetyInventory {
    param([string[]]$Names = @())
    foreach ($row in $script:world) {
        if (-not $Names.Count -or $row.name -in $Names) { $row | ConvertTo-Json -Depth 12 | ConvertFrom-Json }
    }
}
function Invoke-ServiceSafetyStep($Step) {
    if ($Step.name -eq $script:failName) { throw 'Mocked access denied.' }
    $script:writes.Add("$($Step.action):$($Step.name)")
    if ($script:noOp) { return }
    $row = $script:world | Where-Object name -eq $Step.name
    switch ($Step.action) {
        'Stop' { $row.state = 'Stopped'; $row.canStop = $false }
        'Start' { $row.state = 'Running'; $row.canStop = $true }
        'SetStartMode' {
            $row.startMode = if ($Step.startMode -like 'Automatic*') { 'Auto' } else { $Step.startMode }
            $row.delayedAutoStart = $Step.startMode -eq 'AutomaticDelayedStart'
        }
    }
    if ($script:driftAfterStep -and $script:writes.Count -eq 1) { ($script:world | Where-Object name -eq $script:driftAfterStep).binaryPath = 'C:\Changed.exe' }
}
function Assert-Equal($Actual, $Expected, [string]$Message) {
    if (($Actual | ConvertTo-Json -Depth 15 -Compress) -cne ($Expected | ConvertTo-Json -Depth 15 -Compress)) { throw "$Message - actual: $($Actual | ConvertTo-Json -Depth 15 -Compress)" }
}
function Assert-Throws([scriptblock]$Work, [string]$Pattern) {
    try { & $Work } catch { if ($_.Exception.Message -match $Pattern) { return }; throw }
    throw "Expected refusal matching $Pattern"
}
function Reset-World {
    $script:world = @((New-TestService Parent), (New-TestService Child @('Parent')), (New-TestService Leaf @('Parent', 'Child')))
    $script:writes.Clear(); $script:failName = ''; $script:noOp = $false; $script:driftAfterStep = ''
}
try {
    $secureRoot = Join-Path $testRoot 'secure-root'
    Initialize-ServiceSafetyRoot $secureRoot
    $secureAcl = Get-Acl -LiteralPath $secureRoot
    if (-not $secureAcl.AreAccessRulesProtected) { throw 'Service history must disable inherited permissions.' }
    Assert-ServiceSafetyFile $secureRoot
    $userOwned = Join-Path $testRoot 'user-owned.json'
    [IO.File]::WriteAllText($userOwned, '{}')
    $userAcl = Get-Acl -LiteralPath $userOwned
    $userAcl.SetOwner([Security.Principal.WindowsIdentity]::GetCurrent().User)
    Set-Acl -LiteralPath $userOwned -AclObject $userAcl
    Assert-Throws { Assert-ServiceSafetyFile $userOwned } 'unexpected owner'
    $unsafeRoot = Join-Path $testRoot 'unsafe-root'
    Initialize-ServiceSafetyRoot $unsafeRoot
    $unsafeAcl = Get-Acl -LiteralPath $unsafeRoot
    [void]$unsafeAcl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new(
        [Security.Principal.SecurityIdentifier]::new('S-1-1-0'), 'Write', 'ContainerInherit,ObjectInherit', 'None', 'Allow'))
    Set-Acl -LiteralPath $unsafeRoot -AclObject $unsafeAcl
    Assert-Throws { Initialize-ServiceSafetyRoot $unsafeRoot } 'permissions allow changes'
    $junction = Join-Path $testRoot 'redirected-root'
    New-Item -ItemType Junction -Path $junction -Target $secureRoot | Out-Null
    try { Assert-Throws { Initialize-ServiceSafetyRoot $junction } 'links or redirected' }
    finally { [IO.Directory]::Delete($junction) }
    Write-Output 'PASS protected root ACL creation, unsafe write-permission refusal and redirected-folder refusal'
    $firstLock = Enter-ServiceSafetyLock
    try { Assert-Throws { Enter-ServiceSafetyLock } 'in progress' } finally { $firstLock.Dispose() }
    $script:setOwner = (Get-Command Set-ServiceSafetyOwner).ScriptBlock
    function Set-ServiceSafetyOwner { param($Path); throw 'Mocked ownership failure.' }
    $failedWriteId = [guid]::NewGuid().ToString('N')
    Assert-Throws { Write-ServiceSafetyDocument $failedWriteId plan @{ id = $failedWriteId } } 'ownership failure'
    if (Test-Path -LiteralPath ((Get-ServiceSafetyPath $failedWriteId plan) + '.writing')) { throw 'Failed snapshot/plan writes must remove temporary files.' }
    Set-Item -LiteralPath Function:\Set-ServiceSafetyOwner -Value $script:setOwner

    Reset-World
    $plan = New-ServiceSafetyPlan $script:world Stop Parent
    Assert-Equal @($plan.steps.name) @('Leaf', 'Child', 'Parent') 'Stops must be ordered with shared dependents visited once'
    Assert-Equal $script:writes.Count 0 'Preview must never mutate services'
    $restart = New-ServiceSafetyPlan $script:world Restart Parent
    Assert-Equal @($restart.steps | ForEach-Object { "$($_.action):$($_.name)" }) @('Stop:Leaf', 'Stop:Child', 'Stop:Parent', 'Start:Parent', 'Start:Child', 'Start:Leaf') 'Restart must restore dependents in reverse order'
    $script:world[0].dependencies = @('Leaf')
    Assert-Throws { New-ServiceSafetyPlan $script:world Stop Parent } 'circular'
    Reset-World
    $script:world[0].externalDependents = @([pscustomobject]@{ name = 'Driver'; state = 'Running' })
    Assert-Throws { New-ServiceSafetyPlan $script:world Stop Parent } 'driver'
    $script:world = @((New-TestService TermService))
    Assert-Throws { New-ServiceSafetyPlan $script:world Stop TermService } 'safely stopped'
    $script:world = @((New-TestService RpcSs))
    Assert-Throws { New-ServiceSafetyPlan $script:world SetStartMode RpcSs Disabled } 'essential'
    Reset-World
    $script:world[1].state = 'Paused'
    Assert-Throws { New-ServiceSafetyPlan $script:world Stop Parent } 'busy or paused'
    $script:world = @((New-TestService Parent @() Stopped), (New-TestService Child @('Parent') Stopped))
    Assert-Equal @((New-ServiceSafetyPlan $script:world Start Child).steps.name) @('Parent', 'Child') 'Start must include required stopped services'
    $script:world[0].startMode = 'Disabled'
    Assert-Throws { New-ServiceSafetyPlan $script:world Start Child } 'disabled'
    Reset-World
    $modePlan = New-ServiceSafetyPlan $script:world SetStartMode Parent Disabled
    Assert-Equal @($modePlan.steps.action) @('SetStartMode') 'Changing startup must not silently stop a service'
    $script:world = @((New-TestService AppMgmt), (New-TestService DPS), (New-TestService Dependent @('DPS')))
    $profile = New-ServiceSafetyPlan $script:world Profile
    Assert-Equal @($profile.steps | Where-Object action -eq Stop | ForEach-Object name) @('Dependent', 'DPS') 'Profile preview must include dependent stops'
    Write-Output 'PASS dependency ordering, restart restoration, cycles, drivers, protected services, paused states and startup-only changes'

    Reset-World
    $preview = Get-ServiceChangePlan -Action Stop -Name Parent
    $result = Invoke-ServiceChangePlan $preview.id
    Assert-Equal $result.success $true 'Approved plan should complete'
    Assert-Equal @($script:writes) @('Stop:Leaf', 'Stop:Child', 'Stop:Parent') 'Execution must follow preview order'
    $snapshot = Read-ServiceSafetyDocument $result.snapshotId snapshot
    Assert-Equal @($snapshot.services.state) @('Running', 'Running', 'Running') 'Snapshot must capture state before mutation'
    Assert-Throws { Invoke-ServiceChangePlan $preview.id } 'cannot find|does not exist|could not find'
    Reset-World
    $preview = Get-ServiceChangePlan -Action Stop -Name Parent
    $script:world[0].binaryPath = 'C:\Changed.exe'
    Assert-Throws { Invoke-ServiceChangePlan $preview.id } 'changed since the preview'
    Assert-Equal $script:writes.Count 0 'Stale preview must perform no writes'
    Reset-World
    $preview = Get-ServiceChangePlan -Action Stop -Name Parent
    $path = Get-ServiceSafetyPath $preview.id plan
    $document = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
    $document.expiresAt = [DateTime]::UtcNow.AddMinutes(-1).ToString('o')
    $document | ConvertTo-Json -Depth 15 | Set-Content -LiteralPath $path
    Assert-Throws { Invoke-ServiceChangePlan $preview.id } 'expired'
    Assert-Equal $script:writes.Count 0 'Expired preview must perform no writes'
    Reset-World
    $preview = Get-ServiceChangePlan -Action Stop -Name Parent
    Assert-Throws { Invoke-ServiceChangePlan $preview.id -ExpectedAction Start } 'does not match'
    Assert-Equal $script:writes.Count 0 'Mismatched command must perform no writes'
    Assert-Throws { Get-ServiceSafetyPath '../outside' snapshot } 'Invalid'
    $script:writeDocument = (Get-Command Write-ServiceSafetyDocument).ScriptBlock
    function Write-ServiceSafetyDocument {
        param($Id, $Kind, $Document)
        if ($Kind -eq 'snapshot') { throw 'Snapshot disk is full.' }
        & $script:writeDocument -Id $Id -Kind $Kind -Document $Document
    }
    Reset-World
    $preview = Get-ServiceChangePlan -Action Stop -Name Parent
    Assert-Throws { Invoke-ServiceChangePlan $preview.id } 'disk is full'
    Assert-Equal $script:writes.Count 0 'Snapshot failure must prevent every service write'
    Set-Item -LiteralPath Function:\Write-ServiceSafetyDocument -Value $script:writeDocument
    Write-Output 'PASS snapshots precede execution, one-time replay refusal, expiry, stale previews, target binding and identifier confinement'

    Reset-World
    $preview = Get-ServiceChangePlan -Action Stop -Name Parent
    $script:failName = 'Child'
    $result = Invoke-ServiceChangePlan $preview.id
    Assert-Equal $result.success $false 'A refused step must report failure'
    Assert-Equal @($script:writes) @('Stop:Leaf') 'Execution must stop at first failure'
    Assert-Equal @($result.completed.name) @('Leaf') 'Partial result must list verified steps'
    if (-not $result.snapshotId -or $result.failures[0].name -ne 'Child') { throw 'Partial failure lost snapshot or failure details' }
    Reset-World
    $preview = Get-ServiceChangePlan -Action Stop -Name Parent
    $script:driftAfterStep = 'Child'
    $result = Invoke-ServiceChangePlan $preview.id
    Assert-Equal $result.success $false 'Mid-plan drift must stop execution'
    Assert-Equal @($script:writes) @('Stop:Leaf') 'Mid-plan drift must prevent later writes'
    Reset-World
    $preview = Get-ServiceChangePlan -Action SetStartMode -Name Parent -StartMode Manual
    $script:noOp = $true
    $result = Invoke-ServiceChangePlan $preview.id
    Assert-Equal $result.success $false 'Setter success without changed Windows state must fail readback'
    Write-Output 'PASS partial failures, mid-plan drift and authoritative readback'

    Reset-World
    $before = @(Get-ServiceSafetyInventory)
    $script:world[0].startMode = 'Manual'; $script:world[0].state = 'Stopped'
    $diff = Compare-ServiceSafetySnapshots $before @(Get-ServiceSafetyInventory)
    Assert-Equal $diff.configurationCount 1 'Startup change is configuration drift'
    Assert-Equal $diff.runtimeCount 1 'Running state must be counted separately'
    $script:world = @((New-TestService Parent @('b', 'A')))
    $before = @(Get-ServiceSafetyInventory)
    $script:world[0].dependencies = @('a', 'B'); $script:world[0].account = 'LOCALSYSTEM'
    Assert-Equal (Compare-ServiceSafetySnapshots $before @(Get-ServiceSafetyInventory)).changes.Count 0 'Dependency order/case and account case are not drift'
    $script:world[0].delayedAutoStart = $null
    Assert-Equal (Compare-ServiceSafetySnapshots $before @(Get-ServiceSafetyInventory)).unavailableCount 1 'Unreadable settings must not be treated as unchanged'
    $script:world = @((New-TestService SessionCopy))
    $script:world[0].perUserInstance = $true
    $diff = Compare-ServiceSafetySnapshots @() @(Get-ServiceSafetyInventory)
    Assert-Equal $diff.configurationCount 0 'Per-session services are not installation drift'
    Assert-Equal $diff.excludedPerUserCount 1 'Excluded per-user services must be counted'
    $diff = Compare-ServiceSafetySnapshots @((New-TestService Removed)) @((New-TestService Added))
    Assert-Equal $diff.configurationCount 2 'Added and removed services are configuration changes'
    Reset-World
    $manual = New-ServiceSnapshot
    if ($manual.id -notin (Get-ServiceSnapshots).snapshots.id) { throw 'Manual snapshot must be listed' }
    Assert-Equal (Compare-ServiceSnapshot $manual.id).changes.Count 0 'Saved snapshot must compare with live observations'
    Write-Output 'PASS configuration/runtime distinction, unreadable fields, per-user copies, added/removed services and saved snapshot comparison'
} finally {
    $resolvedTestRoot = [IO.Path]::GetFullPath($testRoot)
    if (-not $resolvedTestRoot.StartsWith([IO.Path]::GetFullPath([IO.Path]::GetTempPath()), [StringComparison]::OrdinalIgnoreCase) -or
        [IO.Path]::GetFileName($resolvedTestRoot) -notmatch '^wincom-service-tests-[a-f0-9]{32}$') { throw 'Unsafe test cleanup path.' }
    Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force
}
