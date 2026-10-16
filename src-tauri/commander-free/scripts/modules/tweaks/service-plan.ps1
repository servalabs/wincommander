function New-ServiceSafetyPlan {
    param([array]$Inventory, [ValidateSet('Start', 'Stop', 'Restart', 'SetStartMode', 'Profile')][string]$Action,
        [string]$Name, [ValidateSet('', 'Automatic', 'AutomaticDelayedStart', 'Manual', 'Disabled')][string]$StartMode = '')
    $catalog = @{}
    foreach ($row in $Inventory) { $catalog[$row.name] = $row }
    $steps = [Collections.Generic.List[object]]::new()
    $warnings = [Collections.Generic.List[string]]::new()
    $involved = @{}; $visiting = @{}; $stopped = @{}; $started = @{}
    $protected = @('RpcSs', 'RpcEptMapper', 'DcomLaunch', 'SamSs', 'LSM', 'PlugPlay', 'WinDefend', 'BFE', 'MpsSvc', 'TermService', 'WinCommanderSvc')
    function Add-ServiceStop([string]$Target, [string]$Reason) {
        if ($stopped.ContainsKey($Target)) { return }
        if ($visiting.ContainsKey($Target)) { throw 'Windows reported a circular service dependency. No change will be made.' }
        $row = $catalog[$Target]
        if (-not $row) { throw "Cannot safely inspect service $Target. Drivers cannot be changed here." }
        $involved[$Target] = $row
        if ($row.state -eq 'Stopped') { return }
        if ($row.state -ne 'Running') { throw "$Target is busy or paused. Wait until it is running or stopped, then try again." }
        if ($Target -in $protected -or -not $row.canStop) { throw "$Target cannot be safely stopped by WinCommander." }
        if (@($row.externalDependents | Where-Object state -ne 'Stopped').Count) { throw "$Target has active driver dependents. Drivers cannot be changed here." }
        $visiting[$Target] = $true
        foreach ($dependent in @($Inventory | Where-Object { $Target -in $_.dependencies } | Sort-Object name)) {
            Add-ServiceStop $dependent.name "Depends on $Target"
        }
        [void]$visiting.Remove($Target)
        $steps.Add([pscustomobject][ordered]@{ action = 'Stop'; name = $Target; displayName = $row.displayName; startMode = ''; reason = $Reason })
        $stopped[$Target] = $true
    }
    function Add-ServiceStart([string]$Target, [string]$Reason) {
        if ($started.ContainsKey($Target)) { return }
        if ($visiting.ContainsKey($Target)) { throw 'Windows reported a circular service dependency. No change will be made.' }
        $row = $catalog[$Target]
        if (-not $row) { throw "Cannot safely inspect dependency $Target. Drivers cannot be started here." }
        $involved[$Target] = $row
        if ($row.state -eq 'Running') { return }
        if ($row.state -ne 'Stopped') { throw "$Target is busy or paused. Try again after it settles." }
        if ($row.startMode -eq 'Disabled') { throw "$Target is disabled. Change its startup setting before starting it." }
        if (@($row.externalDependencies | Where-Object state -ne 'Running').Count) { throw "$Target needs a driver that is not running. Drivers cannot be started here." }
        $visiting[$Target] = $true
        foreach ($dependency in @($row.dependencies | Sort-Object)) {
            if ($catalog.ContainsKey($dependency)) { Add-ServiceStart $dependency "Required by $Target" }
        }
        [void]$visiting.Remove($Target)
        $steps.Add([pscustomobject][ordered]@{ action = 'Start'; name = $Target; displayName = $row.displayName; startMode = ''; reason = $Reason })
        $started[$Target] = $true
    }
    function Add-ServiceMode([string]$Target, [string]$Mode) {
        $row = $catalog[$Target]
        if (-not $row) { throw "Service $Target was not found. Drivers cannot be changed here." }
        $involved[$Target] = $row
        if ($Target -in $protected -and $Mode -eq 'Disabled') { throw "$Target is essential and cannot be disabled here." }
        $desired = if ($Mode -like 'Automatic*') { 'Auto' } else { $Mode }
        if ($row.startMode -eq $desired -and ($desired -ne 'Auto' -or $row.delayedAutoStart -eq ($Mode -eq 'AutomaticDelayedStart'))) { return }
        if ($desired -eq 'Auto' -and $null -eq $row.delayedAutoStart) { throw "The delayed-start setting for $Target is unavailable. No change will be made." }
        $steps.Add([pscustomobject][ordered]@{ action = 'SetStartMode'; name = $Target; displayName = $row.displayName; startMode = $Mode; reason = 'Change startup setting' })
    }
    if ($Action -eq 'Profile') {
        $targets = Get-ServiceProfileTargets
        foreach ($target in @($targets.disable)) {
            if ($catalog.ContainsKey($target)) { Add-ServiceStop $target 'Recommended profile' }
        }
        foreach ($target in @($targets.manual)) {
            if ($catalog.ContainsKey($target)) { Add-ServiceMode $target 'Manual' }
        }
        foreach ($target in @($targets.disable)) {
            if ($catalog.ContainsKey($target)) { Add-ServiceMode $target 'Disabled' }
        }
        $warnings.Add('Disabled services and their listed dependents will stop now. Other startup changes apply when Windows next starts them.')
        $warnings.Add('Driver settings are excluded from this service profile.')
    } else {
        if (-not $catalog.ContainsKey($Name)) { throw 'This service no longer exists or is a driver.' }
        switch ($Action) {
            'Start' { Add-ServiceStart $Name 'Requested service' }
            'Stop' { Add-ServiceStop $Name 'Requested service' }
            'Restart' {
                Add-ServiceStop $Name 'Requested service'
                $restore = @($steps.ToArray()); [array]::Reverse($restore)
                foreach ($step in $restore) {
                    if ($catalog[$step.name].startMode -eq 'Disabled') { throw "$($step.name) is disabled and could not be restarted. No change will be made." }
                    $steps.Add([pscustomobject][ordered]@{ action = 'Start'; name = $step.name; displayName = $step.displayName; startMode = ''; reason = 'Restore after restart' })
                }
                if ($restore.Count -eq 0) { Add-ServiceStart $Name 'Requested service' }
            }
            'SetStartMode' {
                if (-not $StartMode) { throw 'Choose a startup setting.' }
                Add-ServiceMode $Name $StartMode
                $warnings.Add('This changes startup only. It does not stop or start the service now.')
            }
        }
    }
    if (@($steps | Where-Object action -eq 'Stop').Count -gt 1) { $warnings.Add('Other services depend on this service. Every affected service is listed below.') }
    return [pscustomobject][ordered]@{
        action = $Action; name = $Name; startMode = $StartMode; steps = @($steps.ToArray()); warnings = @($warnings.ToArray())
        observed = @($involved.Values | Sort-Object name)
    }
}

function Get-ServiceChangePlan {
    param([Parameter(Mandatory)][ValidateSet('Start', 'Stop', 'Restart', 'SetStartMode', 'Profile')][string]$Action,
        [string]$Name = '', [ValidateSet('', 'Automatic', 'AutomaticDelayedStart', 'Manual', 'Disabled')][string]$StartMode = '')
    $ErrorActionPreference = 'Stop'
    Assert-IsAdmin
    foreach ($file in @(Get-ChildItem -LiteralPath (Get-ServiceSafetyRoot) -Filter '*.plan.json' -File -ErrorAction Stop)) {
        $previous = Read-ServiceSafetyDocument $file.Name.Split('.')[0] plan
        if ([DateTime]::Parse($previous.expiresAt).ToUniversalTime() -le [DateTime]::UtcNow) {
            Remove-Item -LiteralPath $file.FullName -ErrorAction Stop
        }
    }
    $plan = New-ServiceSafetyPlan @(Get-ServiceSafetyInventory) $Action $Name $StartMode
    $id = [guid]::NewGuid().ToString('N')
    $expires = [DateTime]::UtcNow.AddMinutes(10).ToString('o')
    if ($plan.steps.Count) {
        Write-ServiceSafetyDocument $id plan ([ordered]@{
            schemaVersion = 1; id = $id; machine = Get-ServiceSafetyMachine; expiresAt = $expires
            owner = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value; plan = $plan
        })
    }
    return @{ id = $id; expiresAt = $expires; action = $Action; name = $Name; steps = $plan.steps; warnings = $plan.warnings }
}
