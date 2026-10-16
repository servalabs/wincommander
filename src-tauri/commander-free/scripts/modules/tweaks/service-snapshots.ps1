function Get-ServiceSafetyInventory {
    param([string[]]$Names = @())
    $ErrorActionPreference = 'Stop'
    Assert-IsAdmin
    $controllers = @{}
    foreach ($controller in @(Get-Service -ErrorAction Stop)) { $controllers[$controller.ServiceName] = $controller }
    try {
        $query = @{ ClassName = 'Win32_Service'; ErrorAction = 'Stop' }
        if ($Names.Count -eq 1) {
            $escapedName = $Names[0].Replace('\', '\\').Replace("'", "\'")
            $query.Filter = "Name = '$escapedName'"
        }
        $rows = foreach ($service in @(Get-CimInstance @query | Sort-Object Name)) {
            if ($Names.Count -and $service.Name -notin $Names) { continue }
            $controller = $controllers[$service.Name]
            if (-not $controller) { throw 'The service list changed while being read. Try again.' }
            $controller.Refresh()
            if (([string]$controller.Status).Replace(' ', '') -ne ([string]$service.State).Replace(' ', '')) { throw 'A service changed state while being read. Try again.' }
            $dependencies = @($controller.ServicesDependedOn)
            $dependents = @($controller.DependentServices)
            [pscustomobject][ordered]@{
                name = [string]$service.Name
                displayName = [string]$service.DisplayName
                startMode = [string]$service.StartMode
                delayedAutoStart = $service.DelayedAutoStart
                state = [string]$service.State
                account = [string]$service.StartName
                binaryPath = [string]$service.PathName
                dependencies = @($dependencies | ForEach-Object ServiceName | Sort-Object)
                externalDependencies = @($dependencies | Where-Object { -not $controllers.ContainsKey($_.ServiceName) } | Sort-Object ServiceName | ForEach-Object { [pscustomobject]@{ name = $_.ServiceName; state = [string]$_.Status } })
                externalDependents = @($dependents | Where-Object { -not $controllers.ContainsKey($_.ServiceName) } | Sort-Object ServiceName | ForEach-Object { [pscustomobject]@{ name = $_.ServiceName; state = [string]$_.Status } })
                canStop = [bool]$controller.CanStop
                perUserInstance = [bool]([int]$controller.ServiceType -band 128)
            }
            foreach ($related in @($dependencies) + @($dependents)) { $related.Dispose() }
        }
        if (@($rows).Count -eq 0) { throw 'Windows returned no services. No change will be made.' }
        return @($rows)
    } finally { foreach ($controller in $controllers.Values) { $controller.Dispose() } }
}

function Save-ServiceSafetySnapshot {
    param([array]$Inventory, [string]$Note = 'Manual snapshot')
    $document = [ordered]@{
        schemaVersion = 1; id = [guid]::NewGuid().ToString('N'); machine = Get-ServiceSafetyMachine
        createdAt = [DateTime]::UtcNow.ToString('o'); note = $Note; services = @($Inventory)
    }
    Write-ServiceSafetyDocument $document.id snapshot $document
    return [pscustomobject]@{ id = $document.id; createdAt = $document.createdAt; note = $Note; serviceCount = $Inventory.Count }
}

function New-ServiceSnapshot {
    Assert-IsAdmin
    $lock = Enter-ServiceSafetyLock
    try { return Save-ServiceSafetySnapshot @(Get-ServiceSafetyInventory) } finally { $lock.Dispose() }
}

function Get-ServiceSnapshots {
    Assert-IsAdmin
    $snapshots = foreach ($file in @(Get-ChildItem -LiteralPath (Get-ServiceSafetyRoot) -Filter '*.snapshot.json' -File -ErrorAction Stop)) {
        $id = $file.Name.Split('.')[0]
        $snapshot = Read-ServiceSafetyDocument $id snapshot
        [pscustomobject]@{ id = $id; createdAt = $snapshot.createdAt; note = $snapshot.note; serviceCount = @($snapshot.services).Count }
    }
    return @{ snapshots = @($snapshots | Sort-Object createdAt -Descending) }
}

function Compare-ServiceSafetySnapshots {
    param([array]$Before, [array]$After)
    $old = @{}; $current = @{}
    foreach ($row in $Before) { $old[$row.name] = $row }
    foreach ($row in $After) { $current[$row.name] = $row }
    $changes = [Collections.Generic.List[object]]::new()
    $excluded = 0
    foreach ($name in @(($old.Keys + $current.Keys) | Sort-Object -Unique)) {
        $left = $old[$name]; $right = $current[$name]
        if (($left -and $left.perUserInstance) -or ($right -and $right.perUserInstance)) { $excluded++; continue }
        if (-not $left -or -not $right) {
            $changes.Add([pscustomobject]@{ name = $name; field = 'Service'; category = 'configuration'; before = $(if ($left) { 'Present' } else { 'Absent' }); after = $(if ($right) { 'Present' } else { 'Absent' }) })
            continue
        }
        foreach ($field in @('startMode', 'delayedAutoStart', 'account', 'binaryPath', 'dependencies', 'state')) {
            $a = $left.$field; $b = $right.$field
            if ($field -eq 'dependencies') { $a = (@($a) | Sort-Object) -join ', '; $b = (@($b) | Sort-Object) -join ', ' }
            $category = if ($field -eq 'state') { 'runtime' } else { 'configuration' }
            if ($null -eq $a -or $null -eq $b) { $category = 'unavailable' }
            $same = if ($field -in @('account', 'dependencies', 'startMode')) { [string]$a -eq [string]$b } else { [string]$a -ceq [string]$b }
            if ($category -eq 'unavailable' -or -not $same) {
                $changes.Add([pscustomobject]@{ name = $name; field = $field; category = $category; before = $a; after = $b })
            }
        }
    }
    return @{
        changes = @($changes.ToArray()); excludedPerUserCount = $excluded
        configurationCount = @($changes | Where-Object category -eq configuration).Count
        runtimeCount = @($changes | Where-Object category -eq runtime).Count
        unavailableCount = @($changes | Where-Object category -eq unavailable).Count
    }
}

function Compare-ServiceSnapshot {
    param([Parameter(Mandatory)][string]$SnapshotId)
    Assert-IsAdmin
    $snapshot = Read-ServiceSafetyDocument $SnapshotId snapshot
    $result = Compare-ServiceSafetySnapshots @($snapshot.services) @(Get-ServiceSafetyInventory)
    $result.snapshot = [pscustomobject]@{ id = $snapshot.id; createdAt = $snapshot.createdAt; note = $snapshot.note }
    return $result
}
