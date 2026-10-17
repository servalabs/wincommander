function Invoke-ServiceSafetyStep {
    param($Step)
    $ErrorActionPreference = 'Stop'
    $controller = [System.ServiceProcess.ServiceController]::new([string]$Step.name)
    try {
        if ($Step.action -eq 'Start') {
            foreach ($dependency in $controller.ServicesDependedOn) {
                try {
                    if ([string]$dependency.Status -ne 'Running') { throw "Dependency $($dependency.ServiceName) changed. Review a fresh preview." }
                } finally { $dependency.Dispose() }
            }
        }
        $arguments = switch ($Step.action) {
            'Stop' { @('stop', [string]$Step.name) }
            'Start' { @('start', [string]$Step.name) }
            'SetStartMode' {
                $mode = switch ($Step.startMode) { 'Automatic' { 'auto' } 'AutomaticDelayedStart' { 'delayed-auto' } 'Manual' { 'demand' } 'Disabled' { 'disabled' } default { throw 'Unsupported startup setting.' } }
                @('config', [string]$Step.name, 'start=', $mode)
            }
            default { throw 'Unsupported service operation.' }
        }
        # sc stop refuses active dependents instead of silently cascading like Stop-Service -Force.
        $output = & (Join-Path ([Environment]::SystemDirectory) 'sc.exe') @arguments 2>&1
        if ($LASTEXITCODE -ne 0) { throw "Windows refused $($Step.action) for $($Step.name): $($output -join ' ')" }
        if ($Step.action -ne 'SetStartMode') {
            $state = if ($Step.action -eq 'Stop') { 'Stopped' } else { 'Running' }
            $controller.WaitForStatus([System.ServiceProcess.ServiceControllerStatus]$state, [TimeSpan]::FromSeconds(30))
        }
    } finally { $controller.Dispose() }
}

function Test-ServiceSafetyObservation {
    param($Before, $After)
    return ($Before | ConvertTo-Json -Depth 12 -Compress) -ceq ($After | ConvertTo-Json -Depth 12 -Compress)
}

function Invoke-ServiceChangePlan {
    param([Parameter(Mandatory)][string]$PlanId, [string]$ExpectedAction = '', [string]$ExpectedName = '', [string]$ExpectedMode = '')
    $ErrorActionPreference = 'Stop'
    Assert-IsAdmin
    $lock = Enter-ServiceSafetyLock
    try {
        $document = Read-ServiceSafetyDocument $PlanId plan
        if ($document.owner -cne [Security.Principal.WindowsIdentity]::GetCurrent().User.Value -or
            [DateTime]::Parse($document.expiresAt).ToUniversalTime() -le [DateTime]::UtcNow) {
            throw 'This preview expired or belongs to another administrator. Review a fresh preview.'
        }
        $plan = $document.plan
        if (($ExpectedAction -and $plan.action -cne $ExpectedAction) -or
            ($ExpectedName -and $plan.name -ine $ExpectedName) -or ($ExpectedMode -and $plan.startMode -cne $ExpectedMode)) {
            throw 'The approved preview does not match this service change.'
        }
        Remove-Item -LiteralPath (Get-ServiceSafetyPath $PlanId plan) -ErrorAction Stop
        $inventory = @(Get-ServiceSafetyInventory)
        $fresh = New-ServiceSafetyPlan $inventory $plan.action $plan.name $plan.startMode
        if (-not (Test-ServiceSafetyObservation $plan $fresh)) { throw 'Services changed since the preview. Nothing was changed. Review a fresh preview.' }
        if (@($plan.steps).Count -eq 0) { return @{ success = $true; status = 'done'; completed = @(); failures = @(); snapshotId = $null; message = 'These service settings are already applied.' } }
        $note = if ($plan.action -eq 'Profile') { 'Before recommended service profile' } else { "Before changing $($plan.name)" }
        $snapshot = Save-ServiceSafetySnapshot $inventory $note
        $expected = @{}
        foreach ($row in $plan.observed) { $expected[$row.name] = $row }
        $completed = [Collections.Generic.List[object]]::new()
        $failures = [Collections.Generic.List[object]]::new()
        foreach ($step in $plan.steps) {
            try {
                $before = @(Get-ServiceSafetyInventory -Names @($step.name))[0]
                if (-not (Test-ServiceSafetyObservation $expected[$step.name] $before)) {
                    throw 'This service changed while the plan was running. Remaining changes were stopped.'
                }
                Invoke-ServiceSafetyStep $step
                $after = @(Get-ServiceSafetyInventory -Names @($step.name))[0]
                if ($step.action -eq 'SetStartMode') {
                    $mode = if ($step.startMode -like 'Automatic*') { 'Auto' } else { $step.startMode }
                    if ($after.startMode -ne $mode -or ($mode -eq 'Auto' -and $after.delayedAutoStart -ne ($step.startMode -eq 'AutomaticDelayedStart'))) { throw 'Windows did not retain the requested startup setting.' }
                } else {
                    $state = if ($step.action -eq 'Stop') { 'Stopped' } else { 'Running' }
                    if ($after.state -ne $state) { throw 'Windows did not reach the requested service state.' }
                }
                $expected[$step.name] = $after
                $completed.Add($step)
            } catch {
                $failures.Add([pscustomobject]@{ name = $step.name; action = $step.action; message = $_.Exception.Message })
                break
            }
        }
        $ok = $failures.Count -eq 0
        $message = if ($ok) { 'Service changes verified. A before-change snapshot was saved.' } else { "$($completed.Count) steps verified; remaining changes stopped. $($failures[0].message) A before-change snapshot is available in Service changes." }
        return @{ success = $ok; status = $(if ($ok) { 'done' } else { 'partial' }); snapshotId = $snapshot.id; completed = @($completed.ToArray()); failures = @($failures.ToArray()); message = $message }
    } finally { $lock.Dispose() }
}
